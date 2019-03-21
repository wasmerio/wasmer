pub type Fd = isize;

#[derive(Debug)]
pub struct Metadata {
    pub len: usize,
    pub is_file: bool,
}

pub trait FileLike {
    /// write
    fn write(&mut self, buf: &[u8], count: usize, offset: usize) -> Result<usize, failure::Error>;
    /// like read(2), will read the data for the file descriptor
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, failure::Error>;
    /// close
    fn close(&self) -> Result<(), failure::Error>;
    // get metadata
    fn metadata(&self) -> Result<Metadata, failure::Error>;
}
