#![cfg(test)]

use std::path::Path;

use super::{block_device::BlockDevice, EmulatedTarget, Request, Target};
use crate::scsi::CmdOutput;

#[test]
fn test_test_unit_ready() {
    let mut target: EmulatedTarget<Vec<u8>, &[u8]> = EmulatedTarget::new();
    let dev = BlockDevice::new(Path::new("/dev/null")).unwrap();
    target.add_lun(Box::new(dev));

    let mut data_in = Vec::new();
    let mut data_out: &[u8] = &[];

    let res = target.execute_command(
        0,
        Request {
            id: 0,
            cdb: &[0, 0, 0, 0, 0, 0],
            task_attr: super::TaskAttr::Simple,
            data_in: &mut data_in,
            data_out: &mut data_out,
            crn: 0,
            prio: 0,
        },
    );

    assert_eq!(res, CmdOutput::ok());
    assert_eq!(&data_in, &[]);
}

#[test]
fn test_report_luns() {
    let mut target: EmulatedTarget<Vec<u8>, &[u8]> = EmulatedTarget::new();
    for _ in 0..5 {
        let dev = BlockDevice::new(Path::new("/dev/null")).unwrap();
        target.add_lun(Box::new(dev));
    }

    let mut data_in = Vec::new();
    let mut data_out: &[u8] = &[];

    let res = target.execute_command(
        0,
        Request {
            id: 0,
            cdb: &[
                0xa0, 0, // reserved
                0, // select report
                0, 0, 0, // reserved
                0, 0, 1, 0, // alloc length: 256
                0, 0,
            ],
            task_attr: super::TaskAttr::Simple,
            data_in: &mut data_in,
            data_out: &mut data_out,
            crn: 0,
            prio: 0,
        },
    );

    assert_eq!(res, CmdOutput::ok());
    assert_eq!(
        &data_in,
        &[
            0, 0, 0, 40, // length: 5*8 = 40
            0, 0, 0, 0, // reserved
            0, 0, 0, 0, 0, 0, 0, 0, // LUN 0
            0, 1, 0, 0, 0, 0, 0, 0, // LUN 1
            0, 2, 0, 0, 0, 0, 0, 0, // LUN 2
            0, 3, 0, 0, 0, 0, 0, 0, // LUN 3
            0, 4, 0, 0, 0, 0, 0, 0, // LUN 4
        ]
    );
}
