pub mod command;
pub mod mode_page;
mod sense;

use std::cmp::min;
use std::convert::TryFrom;
use std::io::{Read, Write};

use crate::{
    image::Image,
    request::VirtioScsiLun,
    scsi::command::{
        Cdb, Command, CommandType, InquiryPageCode, ModePageSelection, ModeSensePageControl,
        ParseError, ReportLunsSelectReport, ReportSupportedOpCodesMode,
    },
    scsi::mode_page::ModePage,
};

use self::sense::SenseTriple;

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum TaskAttr {
    Simple,
    Ordered,
    HeadOfQueue,
    Aca,
}

#[derive(Debug)]
pub struct CmdOutput {
    pub status: u8,
    pub status_qualifier: u16,
    pub sense: Vec<u8>,
}

impl CmdOutput {
    const fn ok() -> Self {
        Self {
            status: 0,
            status_qualifier: 0,
            sense: Vec::new(),
        }
    }
    fn check_condition(sense: SenseTriple) -> Self {
        Self {
            status: 2,
            status_qualifier: 0,
            sense: sense.to_fixed_sense(),
        }
    }
}

struct SilentlyTruncate<W: Write>(W, usize);

impl<W: Write> Write for SilentlyTruncate<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.1 == 0 {
            // our goal is to silently fail, so once we've stopped actually
            // writing, just pretend all writes work
            return Ok(buf.len());
        }
        let len = min(buf.len(), self.1);
        let buf = &buf[..len];
        let written = self.0.write(buf)?;
        self.1 -= written;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

#[repr(u8)] // actually 5 bits
#[allow(dead_code)]
enum DeviceType {
    DirectAccessBlock = 0x0,
    SequentialAccess = 0x1,
    Processor = 0x3,
    CdDvd = 0x5,
    OpticalMemory = 0x7,
    MediaChanger = 0x8,
    StorageArrayController = 0xc,
    EnclosureServices = 0xd,
    SimplifiedDirectAccess = 0xe,
    OpticalCardReaderWriter = 0xf,
    ObjectBasedStorage = 0x11,
}

pub struct Request<'a, W: Write, R: Read> {
    pub lun: VirtioScsiLun,
    pub id: u64,
    pub cdb: &'a [u8],
    pub task_attr: TaskAttr,
    pub data_in: &'a mut W,
    pub data_out: &'a mut R,
    pub crn: u8,
    pub prio: u8,
}

// TODO: would this be more readable split into functions? I lean towards
// thinking it just adds boilderplate, but not sure
#[allow(clippy::too_many_lines)]
pub fn execute_command(req: Request<'_, impl Write, impl Read>, image: &mut Image) -> CmdOutput {
    // dbg!(lun, id, task_attr, crn, prio);
    hope!(req.lun == VirtioScsiLun::TargetLun(0, 0));
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
            // TODO: actually understand the LUN format
            let luns: Vec<[u8; 8]> = vec![[0, 0, 0, 0, 0, 0, 0, 0]];

            hope!(select_report == ReportLunsSelectReport::NoWellKnown);

            data_in.write_all(&8_u32.to_be_bytes()).unwrap();
            data_in.write_all(&[0; 4]).unwrap();
            for lun in luns {
                data_in.write_all(&lun).unwrap();
            }

            CmdOutput::ok()
        }
        Command::ReadCapacity16 => {
            let final_block: u64 = image.size_in_blocks() - 1;
            let block_size: u32 = image.block_size();

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
                    0b1000_0000,   // write protected, no DPOFUA support
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

            let bytes = image
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
                    InquiryPageCode::SupportedVpdPages => {
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
                        0,   // various bits: not removable, not part of a conglomerate, no info on hotpluggability
                        0x7, // version: SPC-6
                        0b0011_0000 | 0x2, // bits: support NormACA, modern LUN format; INQUIRY data version 2
                        91,                // additional INQURIY data length
                        0,                 // don't support various things
                        0,                 // more things we don't have
                        0b0000_0010,       // support command queueing
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
