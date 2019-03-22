use crate::vfs::file_like::{FileLike, Metadata};
use failure::Error;

pub struct Stdin;
pub struct Stdout;
pub struct Stderr;

impl FileLike for Stdin {
    fn write(&mut self, _buf: &[u8], count: usize, _offset: usize) -> Result<usize, Error> {
        println!("writing to {} byte to dev stream...", count);
        Ok(count)
    }

    fn read(&mut self, _buf: &mut [u8]) -> Result<usize, Error> {
        unimplemented!()
    }

    fn close(&self) -> Result<(), Error> {
        Ok(())
    }

    fn metadata(&self) -> Result<Metadata, Error> {
        unimplemented!()
    }
}
