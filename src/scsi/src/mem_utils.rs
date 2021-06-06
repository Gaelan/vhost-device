use std::cmp::min;

use vm_memory::{guest_memory, Address, Bytes, GuestAddress, GuestAddressSpace};

pub struct GuestMemoryRange<M: GuestAddressSpace> {
    address_space: M,
    start: GuestAddress,
    len: u64,
}

impl<M: GuestAddressSpace> Bytes<u64> for GuestMemoryRange<M> {
    type E = guest_memory::Error;

    fn write(&self, buf: &[u8], addr: u64) -> Result<usize, Self::E> {
        let len = min(buf.len() as u64, self.len - addr);
        let buf = &buf[..(len as usize)];
        self.address_space
            .memory()
            .write(buf, self.start.checked_add(addr).unwrap())
    }

    fn read(&self, buf: &mut [u8], addr: u64) -> Result<usize, Self::E> {
        let len = min(buf.len() as u64, self.len - addr);
        let buf = &mut buf[..(len as usize)];
        self.address_space
            .memory()
            .read(buf, self.start.checked_add(addr).unwrap())
    }

    fn write_slice(&self, buf: &[u8], addr: u64) -> Result<(), Self::E> {
        if addr + (buf.len() as u64) >= self.len {
            return guest_memory::Error::InvalidBackendAddress;
        }
        self.address_space
            .memory()
            .write_slice(buf, self.start.checked_add(addr).unwrap())
    }

    fn read_slice(&self, buf: &mut [u8], addr: u64) -> Result<(), Self::E> {
        if (addr + (buf.len() as u64) >= self.len) {
            return guest_memory::Error::InvalidBackendAddress;
        }
        self.address_space
            .memory()
            .read_slice(buf, self.start.checked_add(addr).unwrap())
    }

    fn read_from<F>(&self, addr: u64, src: &mut F, count: usize) -> Result<usize, Self::E>
    where
        F: std::io::Read,
    {
        let count = 
    }

    fn read_exact_from<F>(&self, addr: A, src: &mut F, count: usize) -> Result<(), Self::E>
    where
        F: std::io::Read,
    {
        todo!()
    }

    fn write_to<F>(&self, addr: A, dst: &mut F, count: usize) -> Result<usize, Self::E>
    where
        F: std::io::Write,
    {
        todo!()
    }

    fn write_all_to<F>(&self, addr: A, dst: &mut F, count: usize) -> Result<(), Self::E>
    where
        F: std::io::Write,
    {
        todo!()
    }

    fn store<T: vm_memory::AtomicAccess>(
        &self,
        val: T,
        addr: A,
        order: std::sync::atomic::Ordering,
    ) -> Result<(), Self::E> {
        todo!()
    }

    fn load<T: vm_memory::AtomicAccess>(
        &self,
        addr: A,
        order: std::sync::atomic::Ordering,
    ) -> Result<T, Self::E> {
        todo!()
    }
}
