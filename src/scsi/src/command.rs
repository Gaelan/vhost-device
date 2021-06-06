use crate::scsi::mode_page::ModePage;
use num_enum::TryFromPrimitive;
use std::convert::{TryFrom, TryInto};

#[derive(PartialEq, Eq, TryFromPrimitive, Debug, Copy, Clone)]
#[repr(u8)]
pub enum ReportLunsSelectReport {
    NoWellKnown = 0x0,
    WellKnownOnly = 0x1,
    All = 0x2,
}

#[derive(PartialEq, Eq, Debug, Copy, Clone)]
pub enum InquiryPageCode {
    Ascii(u8),
    Ata,                        // *
    BlockDeviceCharacteristics, // *
    BlockDeviceCharacteristicsExt,
    BlockLimits, // *
    BlockLimitsExt,
    CfaProfile,
    DeviceConstituents,
    DeviceIdentification, // *
    ExtendedInquiry,
    FormatPresets,
    LogicalBlockProvisioning, // *
    ManagementNetworkAddresses,
    ModePagePolicy,
    PowerCondition,
    PowerConsumption,
    PortocolSpecificLogicalUnit,
    ProtocolSpecificPort,
    Referrals,
    ScsiFeatureSets,
    ScsiPorts,
    SoftwareInterfaceIdentification,
    SupportedVpdPages, // *
    ThirdPartyCopy,
    UnitSerialNumber,                // *
    ZonedBlockDeviceCharacteristics, // *
}
// starred ones are ones Linux will use if availible

#[derive(PartialEq, Eq, TryFromPrimitive, Debug, Copy, Clone)]
#[repr(u8)]
pub enum ModeSensePageControl {
    Current = 0b00,
    Changeable = 0b01,
    Default = 0b10,
    Saved = 0b11,
}

impl TryFrom<u8> for InquiryPageCode {
    type Error = ();

    fn try_from(val: u8) -> Result<Self, ()> {
        match val {
            0x00 => Ok(Self::SupportedVpdPages),
            0x1..=0x7f => Ok(Self::Ascii(val)),
            0x80 => Ok(Self::UnitSerialNumber),
            0x83 => Ok(Self::DeviceIdentification),
            0x84 => Ok(Self::SoftwareInterfaceIdentification),
            0x85 => Ok(Self::ManagementNetworkAddresses),
            0x86 => Ok(Self::ExtendedInquiry),
            0x87 => Ok(Self::ModePagePolicy),
            0x88 => Ok(Self::ScsiPorts),
            0x89 => Ok(Self::Ata),
            0x8a => Ok(Self::PowerCondition),
            0x8b => Ok(Self::DeviceConstituents),
            0x8c => Ok(Self::CfaProfile),
            0x8d => Ok(Self::PowerConsumption),
            0x8f => Ok(Self::ThirdPartyCopy),
            0x90 => Ok(Self::PortocolSpecificLogicalUnit),
            0x91 => Ok(Self::ProtocolSpecificPort),
            0x92 => Ok(Self::ScsiFeatureSets),
            0xb0 => Ok(Self::BlockLimits),
            0xb1 => Ok(Self::BlockDeviceCharacteristics),
            0xb2 => Ok(Self::LogicalBlockProvisioning),
            0xb3 => Ok(Self::Referrals),
            0xb5 => Ok(Self::BlockDeviceCharacteristicsExt),
            0xb6 => Ok(Self::ZonedBlockDeviceCharacteristics),
            0xb7 => Ok(Self::BlockLimitsExt),
            0xb8 => Ok(Self::FormatPresets),
            _ => Err(()),
        }
    }
}

impl From<InquiryPageCode> for u8 {
    fn from(pc: InquiryPageCode) -> Self {
        match pc {
            InquiryPageCode::Ascii(val) => val,
            InquiryPageCode::Ata => 0x89,
            InquiryPageCode::BlockDeviceCharacteristics => 0xb1,
            InquiryPageCode::BlockDeviceCharacteristicsExt => 0xb5,
            InquiryPageCode::BlockLimits => 0xb0,
            InquiryPageCode::BlockLimitsExt => 0xb7,
            InquiryPageCode::CfaProfile => 0x8c,
            InquiryPageCode::DeviceConstituents => 0x8b,
            InquiryPageCode::DeviceIdentification => 0x83,
            InquiryPageCode::ExtendedInquiry => 0x86,
            InquiryPageCode::FormatPresets => 0xb8,
            InquiryPageCode::LogicalBlockProvisioning => 0xb2,
            InquiryPageCode::ManagementNetworkAddresses => 0x85,
            InquiryPageCode::ModePagePolicy => 0x87,
            InquiryPageCode::PowerCondition => 0x8a,
            InquiryPageCode::PowerConsumption => 0x8d,
            InquiryPageCode::PortocolSpecificLogicalUnit => 0x90,
            InquiryPageCode::ProtocolSpecificPort => 0x91,
            InquiryPageCode::Referrals => 0xb3,
            InquiryPageCode::ScsiFeatureSets => 0x92,
            InquiryPageCode::ScsiPorts => 0x88,
            InquiryPageCode::SoftwareInterfaceIdentification => 0x84,
            InquiryPageCode::SupportedVpdPages => 0x00,
            InquiryPageCode::ThirdPartyCopy => 0x8f,
            InquiryPageCode::UnitSerialNumber => 0x80,
            InquiryPageCode::ZonedBlockDeviceCharacteristics => 0xb6,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ModePageSelection {
    AllPageZeros,
    Single(ModePage),
}

#[derive(Debug)]
pub enum Command {
    TestUnitReady,
    ReportLuns(ReportLunsSelectReport),
    ReadCapacity16,
    ModeSense6 {
        pc: ModeSensePageControl,
        mode_page: ModePageSelection,
        dbd: bool,
    },
    Inquiry(Option<InquiryPageCode>),
    ReportSupportedOperationCodes {
        rctd: bool,
        mode: ReportSupportedOpCodesMode,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum CommandType {
    TestUnitReady,
    ReportLuns,
    ReadCapacity16,
    ModeSense6,
    Inquiry,
    ReportSupportedOperationCodes,
}

const OPCODES: &[(CommandType, (u8, Option<u16>))] = &[
    (CommandType::TestUnitReady, (0x0, None)),
    (CommandType::Inquiry, (0x12, None)),
    (CommandType::ModeSense6, (0x1a, None)),
    (CommandType::ReadCapacity16, (0x9e, Some(0x10))),
    (CommandType::ReportLuns, (0xa0, None)),
    (
        CommandType::ReportSupportedOperationCodes,
        (0xa3, Some(0xc)),
    ),
];

impl CommandType {
    pub fn from_opcode_and_sa(cmd_opcode: u8, cmd_sa: u16) -> Result<Self, ParseError> {
        OPCODES
            .iter()
            .find(|(_, opcode)| match *opcode {
                (opcode, None) => cmd_opcode == opcode,
                (opcode, Some(sa)) => cmd_opcode == opcode && cmd_sa == sa,
            })
            .map(|&(ty, _)| ty)
            .ok_or_else(|| {
                // This is a little weird: it's usually InvalidCommand, but
                // it's a valid opcode and invalid service action, that's
                // InvalidField
                let mut opcodes = OPCODES.iter().map(|(_, opcode)| opcode);
                let is_invalid_sa = opcodes.any(|&(opcode, _)| opcode == cmd_opcode);
                if is_invalid_sa {
                    ParseError::InvalidField
                } else {
                    ParseError::InvalidCommand
                }
            })
    }
    fn from_cdb(cdb: &[u8]) -> Result<Self, ParseError> {
        Self::from_opcode_and_sa(cdb[0], u16::from(cdb[1] & 0b0001_1111)).map_err(|e| {
            dbg!(cdb);
            e
        })
    }
    /// Return the SCSI "CDB usage data" (see SPC-6 6.34.3) for this command
    /// type.
    ///
    /// Basically, this consists of a structure the size of the CDB for the
    /// command, starting with the opcode and service action (if any), then
    /// proceeding to a bitmap of fields we recognize.
    pub const fn cdb_template(self) -> &'static [u8] {
        match self {
            CommandType::TestUnitReady => &[
                0x0,
                0b0000_0000,
                0b0000_0000,
                0b0000_0000,
                0b0000_0000,
                0b0000_0100,
            ],
            CommandType::ReportLuns => &[
                0xa0,
                0b0000_0000,
                0b1111_1111,
                0b0000_0000,
                0b0000_0000,
                0b0000_0000,
                0b1111_1111,
                0b1111_1111,
                0b1111_1111,
                0b1111_1111,
                0b0000_0000,
                0b0000_0100,
            ],
            CommandType::ReadCapacity16 => &[
                0x9e,
                0x10,
                0b0000_0000,
                0b0000_0000,
                0b0000_0000,
                0b0000_0000,
                0b0000_0000,
                0b0000_0000,
                0b0000_0000,
                0b0000_0000,
                0b1111_1111,
                0b1111_1111,
                0b1111_1111,
                0b1111_1111,
                0b0000_0000,
                0b0000_0100,
            ],
            CommandType::ModeSense6 => &[
                0x1a,
                0b0000_1000,
                0b1111_1111,
                0b1111_1111,
                0b1111_1111,
                0b0000_0100,
            ],
            CommandType::Inquiry => &[
                0x12,
                0b0000_0001,
                0b1111_1111,
                0b1111_1111,
                0b1111_1111,
                0b0000_0100,
            ],
            CommandType::ReportSupportedOperationCodes => &[
                0xa3,
                0xc,
                0b1000_0111,
                0b1111_1111,
                0b1111_1111,
                0b1111_1111,
                0b1111_1111,
                0b1111_1111,
                0b1111_1111,
                0b1111_1111,
                0b0000_0000,
                0b0000_0100,
            ],
        }
    }
}

#[derive(Debug)]
pub struct Cdb {
    pub command: Command,
    pub allocation_length: Option<u32>,
    pub naca: bool,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum ParseError {
    InvalidCommand,
    InvalidField,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum ReportSupportedOpCodesMode {
    All,
    OneCommand(u8),
    OneServiceAction(u8, u16),
    OneCommandOrServiceAction(u8, u16),
}

impl Cdb {
    // TODO: figure out what we're supposed to do for too-short CDBs and
    // start doing that
    // TODO: do we want to ensure reserved fields are 0? SCSI allows, but
    // doesn't require, us to do so.
    // #[deny(clippy::clippy::indexing_slicing)]
    pub fn parse(buf: &[u8]) -> Result<Self, ParseError> {
        let ct = CommandType::from_cdb(buf)?;
        match ct {
            CommandType::TestUnitReady => {
                // TEST UNIT READY
                Ok(Self {
                    command: Command::TestUnitReady,
                    allocation_length: None,
                    naca: (buf[5] & 0b0000_0100) != 0,
                })
            }
            CommandType::Inquiry => {
                // INQUIRY
                let evpd = match buf[1] {
                    0 => false,
                    1 => true,
                    // obselete or reserved bits set
                    _ => return Err(ParseError::InvalidField),
                };
                let page_code_raw = buf[2];
                let page_code = match (evpd, page_code_raw) {
                    (false, 0) => None,
                    (true, pc) => Some(dbg!(pc).try_into().map_err(|_| ParseError::InvalidField)?),
                    (false, _) => return Err(ParseError::InvalidField),
                };
                Ok(Self {
                    command: Command::Inquiry(page_code),
                    allocation_length: Some(u32::from(u16::from_be_bytes(
                        buf[3..5].try_into().unwrap(),
                    ))),
                    naca: (buf[5] & 0b0000_0100) != 0,
                })
            }
            CommandType::ModeSense6 => {
                // MODE SENSE(6)
                let dbd = match buf[1] {
                    0b0000_1000 => true,
                    0b0000_0000 => false,
                    _ => return Err(ParseError::InvalidField),
                };
                let pc = (buf[2] & 0b1100_0000) >> 6;
                let page_code = buf[2] & 0b0011_1111;
                let subpage_code = buf[3];
                let mode: ModePageSelection = match (page_code, subpage_code) {
                    (0x8, 0x0) => ModePageSelection::Single(ModePage::Caching),
                    (0x3f, 0x0) => ModePageSelection::AllPageZeros,
                    _ => {
                        dbg!(page_code, subpage_code);
                        return Err(ParseError::InvalidField);
                    }
                };
                Ok(Self {
                    command: Command::ModeSense6 {
                        pc: pc.try_into().map_err(|_| ParseError::InvalidField)?,
                        mode_page: mode,
                        dbd,
                    },
                    allocation_length: Some(u32::from(buf[4])),
                    naca: (buf[5] & 0b0000_0100) != 0,
                })
            }
            CommandType::ReadCapacity16 => {
                // READ CAPACITY (16)
                Ok(Self {
                    command: Command::ReadCapacity16,
                    allocation_length: Some(u32::from_be_bytes(buf[10..14].try_into().unwrap())),
                    naca: (buf[15] & 0b0000_0100) != 0,
                })
            }
            CommandType::ReportLuns => {
                // REPORT LUNS
                Ok(Self {
                    command: Command::ReportLuns(
                        buf[2].try_into().map_err(|_| ParseError::InvalidField)?,
                    ),
                    allocation_length: Some(u32::from_be_bytes(buf[6..10].try_into().unwrap())),
                    naca: (buf[9] & 0b0000_0100) != 0,
                })
            }
            CommandType::ReportSupportedOperationCodes => {
                // REPORT SUPPORTED OPERATION CODES
                let rctd = buf[2] & 0b1000_0000 != 0;
                let mode = match buf[2] & 0b0000_0111 {
                    0b000 => ReportSupportedOpCodesMode::All,
                    0b001 => ReportSupportedOpCodesMode::OneCommand(buf[3]),
                    0b010 => ReportSupportedOpCodesMode::OneServiceAction(
                        buf[3],
                        u16::from_be_bytes(buf[4..6].try_into().unwrap()),
                    ),
                    0b011 => ReportSupportedOpCodesMode::OneCommandOrServiceAction(
                        buf[3],
                        u16::from_be_bytes(buf[4..6].try_into().unwrap()),
                    ),
                    _ => return Err(ParseError::InvalidField),
                };

                Ok(Self {
                    command: Command::ReportSupportedOperationCodes { rctd, mode },
                    allocation_length: Some(u32::from_be_bytes(buf[6..10].try_into().unwrap())),

                    naca: (buf[11] & 0b0000_0100) != 0,
                })
            }
        }
    }
}
