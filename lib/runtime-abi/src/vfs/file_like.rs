use std::io;

pub type Fd = isize;

#[derive(Debug)]
pub struct Metadata {
    pub len: usize,
    pub is_file: bool,
}

pub trait FileLike: std::io::Read {
    // get metadata
    fn metadata(&self) -> Result<Metadata, failure::Error>;

    // write
    fn write_file(&mut self, buf: &[u8], offset: usize) -> Result<usize, io::Error>;
}
