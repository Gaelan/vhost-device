use std::{
    convert::{TryFrom, TryInto},
    fs::File,
    io::{self, Read, Write},
    os::unix::prelude::*,
    path::Path,
};

use super::EmulatedTarget;
use crate::{
    hope,
    scsi::{
        command::{
            Cdb, Command, CommandType, ModePageSelection, ModeSensePageControl, ParseError,
            ReportLunsSelectReport, ReportSupportedOpCodesMode, VpdPage,
        },
        mode_page::ModePage,
        sense, CmdOutput, DeviceType, LogicalUnit, Request, SilentlyTruncate, TaskAttr,
    },
};

pub struct BlockDevice {
    file: File,
    block_size: u32,
}

impl BlockDevice {
    pub fn new(path: &Path) -> io::Result<Self> {
        // TODO: trying 4096 logical/physical for now. May need to fall
        // back to 512 logical/4096 physical for back compat.
        Ok(Self {
            file: File::open(path)?,
            block_size: 512,
        })
    }

    pub fn read_blocks(&self, lba: u64, blocks: u64) -> io::Result<Vec<u8>> {
        // This is a ton of copies. It should be none.

        let mut ret = vec![0; (blocks * u64::from(self.block_size)) as usize];

        self.file
            .read_exact_at(&mut ret[..], lba * u64::from(self.block_size))?;

        Ok(ret)
    }

    pub fn size_in_blocks(&self) -> u64 {
        let len = self.file.metadata().unwrap().len();
        assert!(len % u64::from(self.block_size) == 0);
        len / u64::from(self.block_size)
    }

    pub const fn block_size(&self) -> u32 {
        self.block_size
    }
}

impl<W: Write, R: Read> LogicalUnit<W, R> for BlockDevice {
    // TODO: would this be more readable split into functions? I lean towards
    // thinking it just adds boilderplate, but not sure
    #[allow(clippy::too_many_lines)]
    fn execute_command(&self, req: Request<'_, W, R>, target: &EmulatedTarget<W, R>) -> CmdOutput {
        // dbg!(lun, id, task_attr, crn, prio);
        hope!(req.crn == 0);
        hope!(req.task_attr == TaskAttr::Simple);
        hope!(req.prio == 0);

        let cdb = match Cdb::parse(req.cdb) {
            Ok(cdb) => cdb,
            Err(ParseError::InvalidCommand) => {
                return CmdOutput::check_condition(sense::INVALID_COMMAND_OPERATION_CODE)
            }
            // TODO: SCSI has a provision for INVALID FIELD IN CDB to include the
            // index of the invalid field, but it's not clear if that's mandatory.
            // In any case, QEMU omits it.
            Err(ParseError::InvalidField) => {
                return CmdOutput::check_condition(sense::INVALID_FIELD_IN_CDB)
            }
        };

        hope!(!cdb.naca);

        let mut data_in = SilentlyTruncate(
            req.data_in,
            cdb.allocation_length.map_or(usize::MAX, |x| x as usize),
        );

        println!("Incoming command: {:?}", &cdb);

        match cdb.command {
            Command::TestUnitReady => CmdOutput::ok(),
            Command::ReportLuns(select_report) => {
                fn encode_lun(lun: u16) -> [u8; 8] {
                    hope!(lun < 256);
                    [0, lun.try_into().unwrap(), 0, 0, 0, 0, 0, 0]
                }
                // TODO: actually understand the LUN format
                // in particular, I think this is wrong over 256 LUNs
                let luns = target.luns().map(encode_lun);

                hope!(select_report == ReportLunsSelectReport::NoWellKnown);

                data_in
                    .write_all(&(u32::try_from(luns.len() * 8)).unwrap().to_be_bytes())
                    .unwrap();
                data_in.write_all(&[0; 4]).unwrap();
                for lun in luns {
                    data_in.write_all(&lun).unwrap();
                }

                CmdOutput::ok()
            }
            Command::ReadCapacity16 => {
                let final_block: u64 = self.size_in_blocks() - 1;
                let block_size: u32 = self.block_size();

                // n.b. this is the last block, ie (length-1), not length
                data_in.write_all(&final_block.to_be_bytes()).unwrap();
                data_in.write_all(&block_size.to_be_bytes()).unwrap();
                // no protection stuff; 1-to-1 logical/physical blocks
                data_in.write_all(&[0, 0]).unwrap();

                // top 2 bits: thin provisioning stuff; other 14 bits are lowest
                // aligned LBA
                data_in.write_all(&[0b1100_0000, 0]).unwrap();

                // reserved
                data_in.write_all(&[0; 16]).unwrap();

                CmdOutput::ok()
            }
            Command::ModeSense6 { mode_page, pc, dbd } => {
                hope!(pc == ModeSensePageControl::Current);
                hope!(!dbd);

                let single_page_array: [ModePage; 1];

                let pages = match mode_page {
                    ModePageSelection::Single(x) => {
                        single_page_array = [x];
                        &single_page_array
                    }
                    ModePageSelection::AllPageZeros => ModePage::ALL_ZERO,
                };

                let pages_len: u32 = pages.iter().map(|x| u32::from(x.page_length() + 2)).sum();
                let pages_len = u8::try_from(pages_len).unwrap();

                // mode parameter header
                data_in
                    .write_all(&[
                        pages_len + 3, // size in bytes after this one
                        0,             // medium type - 0 for SBC
                        0b1000_0000,   // write protected, no DPOFUA support !!!! top bit WP
                        0,             // block desc length
                    ])
                    .unwrap();

                // TODO: block descriptors are optional. does anyone care?

                // // block descriptos
                // // TODO: dynamic size
                // data_in.write_all(&0x1_0000_u32.to_be_bytes()).unwrap();
                // // top byte reserved
                // data_in.write_all(&512_u32.to_be_bytes()).unwrap();

                for page in pages {
                    page.write(&mut data_in);
                }

                CmdOutput::ok()
            }
            Command::Read10 {
                dpo,
                fua,
                lba,
                group_number,
                transfer_length,
            } => {
                hope!(!dpo);
                hope!(!fua);
                hope!(group_number == 0);

                let bytes = self
                    .read_blocks(u64::from(lba), u64::from(transfer_length))
                    .unwrap();

                data_in.write_all(&bytes[..]).unwrap();

                CmdOutput::ok()
            }
            Command::Inquiry(page_code) => {
                // TODO: we should also be responding to INQUIRies to bad LUNs, but
                // right now we terminate those before here

                // top bits 0: peripheral device code = exists and ready
                data_in
                    .write_all(&[DeviceType::DirectAccessBlock as u8])
                    .unwrap();

                if let Some(code) = page_code {
                    let mut out = vec![];
                    match code {
                        VpdPage::SupportedVpdPages => {
                            // TODO: do we want to support other pages?
                            out.push(0);
                        }
                        _ => return CmdOutput::check_condition(sense::INVALID_FIELD_IN_CDB),
                    }
                    data_in.write_all(&[code.into()]).unwrap();
                    data_in
                        .write_all(&u16::try_from(out.len()).unwrap().to_be_bytes())
                        .unwrap();
                    data_in.write_all(&out).unwrap();
                } else {
                    data_in
                        .write_all(&[
                            0,   /* various bits: not removable, not part of a conglomerate, no
                                  * info on hotpluggability */
                            0x7, // version: SPC-6
                            0b0011_0000 | 0x2, /* bits: support NormACA, modern LUN format;
                                  * INQUIRY data version 2 */
                            91,          // additional INQURIY data length
                            0,           // don't support various things
                            0,           // more things we don't have
                            0b0000_0010, // support command queueing
                        ])
                        .unwrap();

                    // TODO: register this or another name with T10
                    // incidentally, QEMU hasn't been registered - they should do that
                    data_in.write_all(b"rust-vmm").unwrap();
                    data_in.write_all(b"vhost-user-scsi ").unwrap();
                    data_in.write_all(b"v0  ").unwrap();
                    // fwiw, the Linux kernel doesn't request any more than this.
                    // no idea if anyone else does.
                    data_in.write_all(&[0; 22]).unwrap();

                    // TODO: are we getting these right? does anyone care?
                    let product_descs: &[u16; 8] = &[0xc0, 0x05c0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0];

                    for desc in product_descs {
                        data_in.write_all(&desc.to_be_bytes()).unwrap();
                    }

                    data_in.write_all(&[0; 22]).unwrap();
                }

                CmdOutput::ok()
            }
            Command::ReportSupportedOperationCodes { rctd, mode } => {
                hope!(!rctd);
                match mode {
                    ReportSupportedOpCodesMode::All => todo!(),
                    ReportSupportedOpCodesMode::OneCommand(cmd) => {
                        let ty = CommandType::from_opcode_and_sa(cmd, 0);
                        data_in.write_all(&[0]).unwrap(); // unused flags
                        if let Ok(ty) = ty {
                            // supported, don't set a bunch of flags
                            data_in.write_all(&[0b0000_0011]).unwrap();
                            let tpl = ty.cdb_template();
                            data_in
                                .write_all(&u16::try_from(tpl.len()).unwrap().to_be_bytes())
                                .unwrap();
                            data_in.write_all(tpl).unwrap();
                        } else {
                            println!("Reporting that we don't support command {:#2x}. It might be worth adding.", cmd);
                            data_in.write_all(&[0b0000_0001]).unwrap(); // not supported
                            data_in.write_all(&[0; 2]).unwrap();
                        }
                        CmdOutput::ok()
                    }
                    ReportSupportedOpCodesMode::OneServiceAction(_, _) => todo!(),
                    ReportSupportedOpCodesMode::OneCommandOrServiceAction(_, _) => {
                        todo!()
                    }
                }
            }
        }
    }
}
