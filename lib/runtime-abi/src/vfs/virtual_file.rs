use failure::Error;

use crate::vfs::file_like::{FileLike, Metadata};

pub struct VirtualFile {
    zbox_file: zbox::File,
}

impl VirtualFile {
    pub fn new(file: zbox::File) -> Self {
        VirtualFile { zbox_file: file }
    }
}

impl FileLike for VirtualFile {
    fn write(&mut self, buf: &[u8], count: usize, offset: usize) -> Result<usize, Error> {
        use std::io::{Seek, SeekFrom};
        self.zbox_file.seek(SeekFrom::Start(offset as u64))?;
        let _ = self.zbox_file.write_once(&buf[..count])?;
        Ok(count)
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        use std::io::Read;
        self.zbox_file.read(buf).map_err(|e| e.into())
    }

    fn close(&self) -> Result<(), Error> {
        Ok(())
    }

    fn metadata(&self) -> Result<Metadata, Error> {
        self.zbox_file
            .metadata()
            .map(|m| Metadata {
                len: m.len(),
                is_file: m.is_file(),
            })
            .map_err(|e: zbox::Error| e.into())
    }
}
