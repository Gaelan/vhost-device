#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(missing_debug_implementations)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::non_ascii_literal)]
mod virtio;
#[macro_use]
mod utils;
// mod mem_utils;
mod scsi;

use std::{
    convert::TryInto,
    io::Read,
    path::PathBuf,
    sync::{Arc, RwLock},
};

use log::info;
use structopt::StructOpt;
use vhost::vhost_user::{
    message::{VhostUserProtocolFeatures, VhostUserVirtioFeatures},
    Listener,
};
use vhost_user_backend::{VhostUserBackend, VhostUserDaemon};
use virtio::VirtioScsiLun;
use virtio_bindings::bindings::virtio_net::VIRTIO_F_VERSION_1;
use vm_memory::{GuestMemoryAtomic, GuestMemoryMmap};

use crate::{
    scsi::{block_device::BlockDevice, EmulatedTarget, TaskAttr},
    virtio::{Response, VirtioScsiResponse},
};

const CDB_SIZE: usize = 32; // TODO: default; can change
const SENSE_SIZE: usize = 96; // TODO: default; can change

// XXX: this type is ridiculous; can we make it less so?
type DescriptorChainWriter = virtio::DescriptorChainWriter<GuestMemoryAtomic<GuestMemoryMmap>>;
type DescriptorChainReader = virtio::DescriptorChainReader<GuestMemoryAtomic<GuestMemoryMmap>>;
type Target = dyn scsi::Target<DescriptorChainWriter, DescriptorChainReader>;

struct VhostUserScsiBackend {
    mem: Option<GuestMemoryAtomic<GuestMemoryMmap>>,
    // image: Mutex<BlockDevice>,
    targets: Vec<Box<Target>>,
}

impl VhostUserScsiBackend {
    fn new() -> Self {
        Self {
            mem: None,
            // image: Mutex::new(image),
            targets: Vec::new(),
        }
    }
}

impl VhostUserScsiBackend {
    fn handle_control_queue(
        &self,
        reader: &mut DescriptorChainReader,
        writer: &mut DescriptorChainWriter,
    ) {
        // dbg!(buf[0]);
        todo!();
    }

    fn parse_target(&self, lun: VirtioScsiLun) -> Option<(&Target, u16)> {
        match lun {
            VirtioScsiLun::TargetLun(target, lun) => self
                .targets
                .get(usize::from(target))
                .map(|tgt| (tgt.as_ref(), lun)),
            // TODO: do we need to handle the REPORT LUNS well-known LUN?
            // In practice, everyone seems to just use LUN 0
            VirtioScsiLun::ReportLuns => None,
        }
    }

    fn handle_request_queue(
        &self,
        reader: &mut DescriptorChainReader,
        writer: &mut DescriptorChainWriter,
    ) {
        let mut buf = [0; 19 + CDB_SIZE];
        reader.read_exact(&mut buf).unwrap();
        // unwrap is safe, we just sliced 8 out
        let lun = VirtioScsiLun::parse(buf[0..8].try_into().unwrap()).unwrap();
        let id = u64::from_le_bytes(buf[8..16].try_into().unwrap());

        let task_attr = match buf[16] {
            0 => TaskAttr::Simple,
            1 => TaskAttr::Ordered,
            2 => TaskAttr::HeadOfQueue,
            3 => TaskAttr::Aca,
            _ => todo!(),
        };
        let prio = buf[17];
        let crn = buf[18];
        let cdb = &buf[19..(19 + CDB_SIZE)];

        let mut body_writer = writer.clone();
        body_writer.skip(108); // header + 96 (default sense size)

        let response = if let Some((target, lun)) = self.parse_target(lun) {
            let output = target.execute_command(
                lun,
                scsi::Request {
                    id,
                    cdb,
                    task_attr,
                    data_in: &mut body_writer,
                    data_out: reader,
                    crn,
                    prio,
                },
            );

            println!("Command result: {:?}", output.status);

            assert!(output.sense.len() < SENSE_SIZE);

            Response {
                response: VirtioScsiResponse::Ok,
                status: output.status,
                status_qualifier: output.status_qualifier,
                sense: output.sense,
                // TODO: handle residual for data in
                residual: body_writer.residual(),
            }
        } else {
            println!("Rejecting command to {:?}", lun);
            Response {
                response: VirtioScsiResponse::BadTarget,
                status: 0,
                status_qualifier: 0,
                sense: Vec::new(),
                residual: body_writer.residual(),
            }
        };

        // dbg!(body_writer.written);
        // hope!(body_writer.done());

        response.write(writer).unwrap();
    }

    fn add_target(&mut self, target: Box<Target>) {
        self.targets.push(target);
    }
}

impl VhostUserBackend for VhostUserScsiBackend {
    fn num_queues(&self) -> usize {
        let num_request_queues = 1;
        2 + num_request_queues
    }

    fn max_queue_size(&self) -> usize {
        128 // qemu assumes this by default
    }

    fn features(&self) -> u64 {
        // TODO: Any other ones worth implementing? EVENT_IDX and INDIRECT_DESC
        // are supported by virtiofsd
        1 << VIRTIO_F_VERSION_1 | VhostUserVirtioFeatures::PROTOCOL_FEATURES.bits() | 1 << 2
    }

    fn protocol_features(&self) -> VhostUserProtocolFeatures {
        VhostUserProtocolFeatures::MQ
        // | VhostUserProtocolFeatures::REPLY_ACK
        // | VhostUserProtocolFeatures::SLAVE_REQ
    }

    fn set_event_idx(&mut self, enabled: bool) {
        assert!(!enabled) // should always be true until we support EVENT_IDX in
                          // features
    }

    fn update_memory(
        &mut self,
        atomic_mem: GuestMemoryAtomic<GuestMemoryMmap>,
    ) -> std::result::Result<(), std::io::Error> {
        info!("Memory updated - guest probably booting");
        self.mem = Some(atomic_mem);
        Ok(())
    }

    fn handle_event(
        &self,
        device_event: u16,
        evset: epoll::Events,
        vrings: &[Arc<RwLock<vhost_user_backend::Vring>>],
        thread_id: usize,
    ) -> std::result::Result<bool, std::io::Error> {
        // println!("handle_event: {}", device_event);

        hope!(evset == epoll::Events::EPOLLIN); // TODO: virtiofsd returns an error on this
        hope!(vrings.len() == 3);
        hope!(thread_id == 0);

        hope!((device_event as usize) < vrings.len());
        let mut vring = vrings[device_event as usize].write().unwrap();
        let queue = vring.mut_queue();

        let mut used = Vec::new();

        for dc in queue.iter().unwrap() {
            let mut writer = DescriptorChainWriter::new(dc.clone());
            let mut reader = DescriptorChainReader::new(dc.clone());
            match device_event {
                0 => self.handle_control_queue(&mut reader, &mut writer),
                2 => {
                    // let mut img = self.image.lock().unwrap();
                    self.handle_request_queue(&mut reader, &mut writer)
                }
                _ => todo!(),
            }

            used.push((dc.head_index(), writer.max_written()))
        }

        for (hi, len) in used {
            queue.add_used(hi, len).unwrap();
        }

        vring.signal_used_queue().unwrap();

        // todo!()

        Ok(false) // TODO: what's this bool? no idea. virtiofd-rs returns false
    }

    // fn acked_features(&mut self, features: u64) {
    //     dbg!(features);
    // }

    fn get_config(&self, _offset: u32, _size: u32) -> Vec<u8> {
        // QEMU handles config space itself
        panic!("Access to configuration space is not supported.");
    }

    fn set_config(&mut self, _offset: u32, _buf: &[u8]) -> std::result::Result<(), std::io::Error> {
        // QEMU handles config space itself
        panic!("Access to configuration space is not supported.");
    }

    // fn exit_event(&self, _thread_index: usize) -> Option<(EventFd, Option<u16>)>
    // {     dbg!();
    //     // let fd = EventFd::new(EFD_NONBLOCK).unwrap();
    //     // let ret = Some((fd.try_clone().unwrap(), Some(3)));
    //     // mem::forget(fd);
    //     // ret
    //     None
    // }
}

#[derive(StructOpt, Debug)]
struct Opt {
    /// Make the images read-only.
    ///
    /// Currently, we don't actually support writes, but this is still useful:
    /// if we tell Linux the disk is write-protected, some tools using the SCSI
    /// generic API won't work. But if we don't, it'll try to write to the disk
    /// on mount, and fail.
    #[structopt(long("read-only"), short("r"))]
    read_only: bool,
    /// Tell the guest this disk is non-rotational.
    ///
    /// Affects some heuristics in Linux around, for example, scheduling.
    #[structopt(long("solid-state"), short("s"))]
    solid_state: bool,
    #[structopt(parse(from_os_str))]
    sock: PathBuf,
    #[structopt(parse(from_os_str))]
    images: Vec<PathBuf>,
}

fn main() {
    env_logger::init();

    let opt = Opt::from_args();

    let mut backend = VhostUserScsiBackend::new();
    let mut target = EmulatedTarget::new();

    for image in opt.images {
        let mut dev = BlockDevice::new(&image).expect("opening image");
        dev.set_write_protected(opt.read_only);
        dev.set_solid_state(opt.solid_state);
        target.add_lun(Box::new(dev));
    }

    backend.add_target(Box::new(target));

    let mut daemon = VhostUserDaemon::new("vhost-user-scsi".into(), Arc::new(RwLock::new(backend)))
        .expect("Creating daemon");

    daemon
        .start(Listener::new(opt.sock, true).expect("listener"))
        .expect("starting daemon");

    daemon.wait().expect("waiting");
}
