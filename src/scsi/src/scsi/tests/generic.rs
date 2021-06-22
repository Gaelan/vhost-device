use std::path::Path;

use super::do_command_fail;
use crate::scsi::{block_device::BlockDevice, sense, EmulatedTarget};

#[test]
fn test_invalid_opcode() {
    let mut target: EmulatedTarget<Vec<u8>, &[u8]> = EmulatedTarget::new();
    let dev = BlockDevice::new(Path::new("src/scsi/test.img")).unwrap();
    target.add_lun(Box::new(dev));

    do_command_fail(
        &mut target,
        &[
            0xff, // vendor specific, unused by us
            0, 0, 0, 0, 0,
        ],
        sense::INVALID_COMMAND_OPERATION_CODE,
    );
}

#[test]
fn test_invalid_service_action() {
    let mut target: EmulatedTarget<Vec<u8>, &[u8]> = EmulatedTarget::new();
    let dev = BlockDevice::new(Path::new("src/scsi/test.img")).unwrap();
    target.add_lun(Box::new(dev));

    do_command_fail(
        &mut target,
        &[
            0xa3, // MAINTAINANCE IN
            0x1f, // vendor specific, unused by us
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        sense::INVALID_FIELD_IN_CDB,
    );
}
