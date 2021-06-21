#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub struct SenseTriple(u8, u8, u8);

impl SenseTriple {
    pub fn to_fixed_sense(self) -> Vec<u8> {
        vec![
            0x70,   // response code (fixed, current); valid bit (0)
            0x0,    // reserved
            self.0, // sk; various upper bits 0
            0x0, 0x0, 0x0, 0x0, // information
            0xa, // add'l sense length
            0x0, 0x0, 0x0, 0x0,    // cmd-specific information
            self.1, // asc
            self.2, // ascq
            0x0,    // field-replacable unit code
            0x0, 0x0, 0x0, // sense-key-sepcific information
        ]
    }
}

pub const INVALID_COMMAND_OPERATION_CODE: SenseTriple = SenseTriple(0x5, 0x20, 0x0);
pub const LOGICAL_BLOCK_ADDRESS_OUT_OF_RANGE: SenseTriple = SenseTriple(0x5, 0x21, 0x0);
pub const INVALID_FIELD_IN_CDB: SenseTriple = SenseTriple(0x5, 0x24, 0x0);
