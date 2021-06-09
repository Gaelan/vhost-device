use std::{
    fs::File,
    io::{self},
    os::unix::prelude::*,
    path::Path,
};

pub struct Image {
    file: File,
    block_size: u32,
}

impl Image {
    pub fn new(path: &Path) -> io::Result<Self> {
        // TODO: trying 4096 logical/physical for now. May need to fall
        // back to 512 logical/4096 physical for back compat.
        Ok(Self {
            file: File::open(path)?,
            block_size: 512,
        })
    }

    pub fn read_blocks(&mut self, lba: u64, blocks: u64) -> io::Result<Vec<u8>> {
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
