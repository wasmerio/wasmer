pub type Fd = isize;

#[derive(Debug)]
pub struct Metadata {
    pub len: usize,
    pub is_file: bool,
}

pub trait FileLike: std::io::Write + std::io::Read + std::io::Seek {
    // get metadata
    fn metadata(&self) -> Result<Metadata, failure::Error>;

    // write
    //    fn write_file(&mut self, buf: &[u8]) -> Result<usize, io::Error>;

    // read
    //    fn read_file(&mut self, buf: &mut [u8]) -> Result<usize, io::Error>;

    // set_file_len
    fn set_file_len(&mut self, len: usize) -> Result<(), failure::Error>;
}
