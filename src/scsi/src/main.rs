#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(missing_debug_implementations)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::clippy::module_name_repetitions)]
mod image;
mod request;
#[macro_use]
mod utils;
// mod mem_utils;
mod scsi;

use std::{
    convert::TryInto,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
};

use image::Image;
use request::VirtioScsiLun;
use structopt::StructOpt;
use vhost::vhost_user::{
    message::{VhostUserProtocolFeatures, VhostUserVirtioFeatures},
    Listener,
};
use vhost_user_backend::{VhostUserBackend, VhostUserDaemon};
use virtio_bindings::bindings::virtio_net::VIRTIO_F_VERSION_1;
use vm_memory::{Bytes, GuestAddressSpace, GuestMemoryAtomic, GuestMemoryMmap};
use vmm_sys_util::eventfd::EventFd;

use crate::{
    request::{DescriptorChainReader, DescriptorChainWriter, Response, VirtioScsiResponse},
    scsi::TaskAttr,
};

const CDB_SIZE: usize = 32; // TODO: default; can change
const SENSE_SIZE: usize = 96; // TODO: default; can change

struct VhostUserScsiBackend {
    mem: Option<GuestMemoryAtomic<GuestMemoryMmap>>,
    image: Mutex<Image>,
}

impl VhostUserScsiBackend {
    fn new(image: Image) -> Self {
        Self {
            mem: None,
            image: Mutex::new(image),
        }
    }
}

fn handle_control_queue(
    buf: &[u8],
    writer: &mut DescriptorChainWriter<impl GuestAddressSpace + Clone>,
) {
    dbg!(buf[0]);
    todo!();
}

fn handle_request_queue(
    buf: &[u8],
    writer: &mut DescriptorChainWriter<impl GuestAddressSpace + Clone>,
    image: &mut Image,
) {
    // unwrap is safe unless it's too short - we just sliced 8 out
    // TODO: but it does panic if it's too short
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

    let response = if lun == VirtioScsiLun::TargetLun(0, 0) {
        let output = scsi::execute_command(
            scsi::Request {
                lun,
                id,
                cdb,
                task_attr,
                data_in: &mut body_writer,
                data_out: &mut DescriptorChainReader,
                crn,
                prio,
            },
            image,
        );

        println!("Command result: {:?}", output.status);

        assert!(output.sense.len() < SENSE_SIZE);

        Response {
            response: VirtioScsiResponse::Ok,
            status: output.status,
            status_qualifier: output.status_qualifier,
            sense: output.sense,
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

impl VhostUserBackend for VhostUserScsiBackend {
    fn num_queues(&self) -> usize {
        dbg!();
        let num_request_queues = 1;
        2 + num_request_queues
    }

    fn max_queue_size(&self) -> usize {
        dbg!();
        128 // qemu assumes this by default
    }

    fn features(&self) -> u64 {
        dbg!();
        // TODO: Any other ones worth implementing? EVENT_IDX and INDIRECT_DESC
        // are supported by virtiofsd
        1 << VIRTIO_F_VERSION_1 | VhostUserVirtioFeatures::PROTOCOL_FEATURES.bits() | 1 << 2
    }

    fn protocol_features(&self) -> VhostUserProtocolFeatures {
        dbg!();
        VhostUserProtocolFeatures::MQ
        // | VhostUserProtocolFeatures::REPLY_ACK
        // | VhostUserProtocolFeatures::SLAVE_REQ
    }

    fn set_event_idx(&mut self, enabled: bool) {
        dbg!();
        assert!(!enabled) // should always be true until we support EVENT_IDX in features
    }

    fn update_memory(
        &mut self,
        atomic_mem: GuestMemoryAtomic<GuestMemoryMmap>,
    ) -> std::result::Result<(), std::io::Error> {
        dbg!();
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
            let mem = dc.memory();

            let mut iter = dc.clone().readable();
            let d = iter.next().unwrap();
            hope!(iter.next().is_none());

            let mut s: Vec<u8> = vec![0; d.len() as usize];
            mem.read_slice(&mut s[..], d.addr()).unwrap();

            let mut writer = DescriptorChainWriter::new(dc.clone());
            match device_event {
                0 => handle_control_queue(&s, &mut writer),
                2 => {
                    let mut img = self.image.lock().unwrap();
                    handle_request_queue(&s, &mut writer, &mut *img)
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

        Ok(false) // what's this bool? no idea. virtiofd-rs returns false
    }

    fn acked_features(&mut self, features: u64) {
        dbg!(features);
    }

    fn get_config(&self, _offset: u32, _size: u32) -> Vec<u8> {
        dbg!();
        todo!();
    }

    fn set_config(&mut self, _offset: u32, _buf: &[u8]) -> std::result::Result<(), std::io::Error> {
        dbg!();
        todo!();
    }

    fn exit_event(&self, _thread_index: usize) -> Option<(EventFd, Option<u16>)> {
        dbg!();
        // let fd = EventFd::new(EFD_NONBLOCK).unwrap();
        // let ret = Some((fd.try_clone().unwrap(), Some(3)));
        // mem::forget(fd);
        // ret
        None
    }

    fn set_slave_req_fd(&mut self, _vu_req: vhost::vhost_user::SlaveFsCacheReq) {
        dbg!();
        // mem::forget(vu_req);
    }

    // fn queues_per_thread(&self) -> Vec<u64> {
    //     vec![0xffff_ffff]
    // }
}

#[derive(StructOpt, Debug)]
struct Opt {
    #[structopt(parse(from_os_str))]
    sock: PathBuf,
    #[structopt(parse(from_os_str))]
    image: PathBuf,
}

fn main() {
    env_logger::init();

    let opt = Opt::from_args();

    let img = Image::new(&opt.image).expect("opening image");

    let mut daemon = VhostUserDaemon::new(
        "vhost-user-scsi".into(),
        Arc::new(RwLock::new(VhostUserScsiBackend::new(img))),
    )
    .expect("Creating daemon");

    dbg!();

    daemon
        .start(Listener::new(opt.sock, true).expect("listener"))
        .expect("starting daemon");

    dbg!();

    // daemon.get_vring_workers();

    daemon.wait().expect("waiting");

    dbg!();
}
