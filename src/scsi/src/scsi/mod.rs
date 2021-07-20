pub mod block_device;
pub mod command;
pub mod mode_page;
mod response_data;
mod sense;
mod tests;

use std::{
    cmp::min,
    convert::TryFrom,
    io::{self, Read, Write},
};

use self::{
    command::{Cdb, Command, SenseFormat},
    response_data::respond_standard_inquiry_data,
    sense::SenseTriple,
    CmdError::DataIn,
};
use crate::scsi::{command::ReportLunsSelectReport, response_data::respond_report_luns};

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum TaskAttr {
    Simple,
    Ordered,
    HeadOfQueue,
    Aca,
}

#[derive(Debug, PartialEq, Eq)]
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
    pub id: u64,
    pub cdb: &'a [u8],
    pub task_attr: TaskAttr,
    pub data_in: &'a mut W,
    pub data_out: &'a mut R,
    pub crn: u8,
    pub prio: u8,
}

pub trait Target<W: Write, R: Read>: Send + Sync {
    fn execute_command(&self, lun: u16, req: Request<'_, W, R>) -> Result<CmdOutput, CmdError>;
}

pub trait LogicalUnit<W: Write, R: Read>: Send + Sync {
    /// Process a SCSI command sent to this logical unit.
    ///
    /// # Return value
    /// This function returns a Result, but it should only return Err in one
    /// circumstance: when an attempt to transfer data via `req.data_in` or
    /// `req.data_out` fails, in which case it should pass that error through.
    /// Any other errors, such as invalid SCSI commands or I/O errors
    /// accessing an underlying file, should result in an Ok return value
    /// with a `CmdOutput` representing a SCSI-level error (i.e. CHECK
    /// CONDITION status, and appropriate sense data).
    fn execute_command(
        &self,
        req: Request<'_, W, R>,
        target: &EmulatedTarget<W, R>,
    ) -> Result<CmdOutput, CmdError>;
}

struct MissingLun;

impl<W: Write, R: Read> LogicalUnit<W, R> for MissingLun {
    fn execute_command(
        &self,
        req: Request<'_, W, R>,
        target: &EmulatedTarget<W, R>,
    ) -> Result<CmdOutput, CmdError> {
        let parse = Cdb::parse(req.cdb);

        if let Ok(cdb) = parse {
            let mut data_in = SilentlyTruncate(
                req.data_in,
                cdb.allocation_length.map_or(usize::MAX, |x| x as usize),
            );

            match cdb.command {
                Command::ReportLuns(select_report) => {
                    match select_report {
                        ReportLunsSelectReport::NoWellKnown | ReportLunsSelectReport::All => {
                            respond_report_luns(&mut data_in, target.luns()).map_err(DataIn)?;
                        }
                        ReportLunsSelectReport::WellKnownOnly
                        | ReportLunsSelectReport::Administrative
                        | ReportLunsSelectReport::TopLevel
                        | ReportLunsSelectReport::SameConglomerate => {
                            respond_report_luns(&mut data_in, vec![].into_iter())
                                .map_err(DataIn)?;
                        }
                    }
                    Ok(CmdOutput::ok())
                }
                Command::Inquiry(page_code) => {
                    // peripheral qualifier 0b011: logical unit not accessible
                    // device type 0x1f: unknown/no device type
                    data_in.write_all(&[0b0110_0000 | 0x1f]).map_err(DataIn)?;
                    match page_code {
                        Some(_) => {
                            // SPC-6 7.7.2: "If the PERIPHERAL QUALIFIER field is
                            // not set to 000b, the contents of the PAGE LENGTH
                            // field and the VPD parameters are outside the
                            // scope of this standard."
                            //
                            // Returning a 0 length and no data seems sensible enough.
                            data_in.write_all(&[0]).map_err(DataIn)?;
                        }
                        None => {
                            respond_standard_inquiry_data(&mut data_in).map_err(DataIn)?;
                        }
                    }
                    Ok(CmdOutput::ok())
                }
                Command::RequestSense(format) => {
                    match format {
                        SenseFormat::Fixed => {
                            data_in
                                .write_all(&sense::LOGICAL_UNIT_NOT_SUPPORTED.to_fixed_sense())
                                .map_err(DataIn)?;
                            Ok(CmdOutput::ok())
                        }
                        SenseFormat::Descriptor => {
                            // Don't support desciptor format.
                            Ok(CmdOutput::check_condition(sense::INVALID_FIELD_IN_CDB))
                        }
                    }
                }
                _ => Ok(CmdOutput::check_condition(
                    sense::LOGICAL_UNIT_NOT_SUPPORTED,
                )),
            }
        } else {
            // invalid command - presumably we don't treat these any differently?
            Ok(CmdOutput::check_condition(
                sense::LOGICAL_UNIT_NOT_SUPPORTED,
            ))
        }
    }
}

pub struct EmulatedTarget<W: Write, R: Read> {
    luns: Vec<Box<dyn LogicalUnit<W, R>>>,
}

impl<W: Write, R: Read> EmulatedTarget<W, R> {
    pub fn new() -> Self {
        Self { luns: Vec::new() }
    }

    pub fn add_lun(&mut self, logical_unit: Box<dyn LogicalUnit<W, R>>) {
        self.luns.push(logical_unit);
    }

    pub fn luns(&self) -> impl Iterator<Item = u16> + ExactSizeIterator + '_ {
        self.luns
            .iter()
            .enumerate()
            .map(|(idx, _logical_unit)| u16::try_from(idx).unwrap())
    }
}

impl<W: Write, R: Read> Target<W, R> for EmulatedTarget<W, R> {
    fn execute_command(&self, lun: u16, req: Request<'_, W, R>) -> Result<CmdOutput, CmdError> {
        let lun: &dyn LogicalUnit<W, R> = self
            .luns
            .get(lun as usize)
            .map_or(&MissingLun, |x| x.as_ref());

        lun.execute_command(req, self)
    }
}

/// An transport-level error encountered while processing a SCSI command.
///
/// This is only for transport-level errors; anything else should be handled by
/// returning a CHECK CONDITION status at the SCSI level.
#[derive(Debug)]
pub enum CmdError {
    /// The provided CDB is too short for its operation code.
    CdbTooShort,
    /// An error occurred while writing to the provided data in writer.
    DataIn(io::Error),
}
