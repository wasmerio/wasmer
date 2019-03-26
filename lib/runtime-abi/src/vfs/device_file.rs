use crate::vfs::file_like::{FileLike, Metadata};
use failure::Error;
use std::io;
use std::io::{Read, Write};

pub struct Stdin;
pub struct Stdout;
pub struct Stderr;

impl FileLike for Stdin {
    fn metadata(&self) -> Result<Metadata, Error> {
        unimplemented!()
    }

    fn write_file(&mut self, _buf: &[u8], _offset: usize) -> Result<usize, io::Error> {
        unimplemented!()
    }
}

impl Read for Stdin {
    fn read(&mut self, _buf: &mut [u8]) -> Result<usize, io::Error> {
        unimplemented!()
    }
}

impl FileLike for Stdout {
    fn metadata(&self) -> Result<Metadata, failure::Error> {
        unimplemented!()
    }

    fn write_file(&mut self, buf: &[u8], _offset: usize) -> Result<usize, io::Error> {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        handle.write(buf)
    }
}

impl Read for Stdout {
    fn read(&mut self, _buf: &mut [u8]) -> Result<usize, io::Error> {
        unimplemented!()
    }
}

impl FileLike for Stderr {
    fn metadata(&self) -> Result<Metadata, failure::Error> {
        unimplemented!()
    }

    fn write_file(&mut self, buf: &[u8], _offset: usize) -> Result<usize, io::Error> {
        let stderr = io::stderr();
        let mut handle = stderr.lock();
        handle.write(buf)
    }
}

impl Read for Stderr {
    fn read(&mut self, _buf: &mut [u8]) -> Result<usize, io::Error> {
        unimplemented!()
    }
}
