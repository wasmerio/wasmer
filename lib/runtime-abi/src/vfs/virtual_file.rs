use crate::vfs::file_like::{FileLike, Metadata};
use failure::Error;
use std::io;

pub struct VirtualFile(zbox::File);

impl VirtualFile {
    pub fn new(file: zbox::File) -> Self {
        VirtualFile(file)
    }
}

impl FileLike for VirtualFile {
    fn metadata(&self) -> Result<Metadata, Error> {
        self.0
            .metadata()
            .map(|m| Metadata {
                len: m.content_len(),
                is_file: m.is_file(),
            })
            .map_err(|e: zbox::Error| e.into())
    }

    fn set_file_len(&mut self, len: usize) -> Result<(), failure::Error> {
        self.0.set_len(len).map_err(|e| e.into())
    }
}

impl io::Write for VirtualFile {
    fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        let result = self.0.write(buf)?;
        self.0.finish().unwrap();
        Ok(result)
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        self.0.flush()
    }
}

impl io::Read for VirtualFile {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        self.0.read(buf)
    }
}

impl io::Seek for VirtualFile {
    fn seek(&mut self, pos: io::SeekFrom) -> Result<u64, io::Error> {
        self.0.seek(pos)
    }
}
