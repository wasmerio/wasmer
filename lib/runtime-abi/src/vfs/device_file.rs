use crate::vfs::file_like::{FileLike, Metadata};
use failure::Error;

pub struct Stdin;
pub struct Stdout;
pub struct Stderr;

impl FileLike for Stdin {
    fn write(&mut self, _buf: &[u8], _count: usize, _offset: usize) -> Result<usize, Error> {
        unimplemented!()
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
