use crate::vfs::file_like::{FileLike, Metadata};
use failure::Error;
use std::io;

pub struct Stdin;
pub struct Stdout;
pub struct Stderr;

impl FileLike for Stdin {
    fn metadata(&self) -> Result<Metadata, Error> {
        unimplemented!()
    }

    fn set_file_len(&mut self, _len: usize) -> Result<(), failure::Error> {
        panic!("Cannot set length of stdin");
    }
}

impl io::Read for Stdin {
    fn read(&mut self, _buf: &mut [u8]) -> Result<usize, io::Error> {
        unimplemented!()
    }
}

impl io::Write for Stdin {
    fn write(&mut self, _buf: &[u8]) -> Result<usize, io::Error> {
        unimplemented!()
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        unimplemented!()
    }
}

impl io::Seek for Stdin {
    fn seek(&mut self, _pos: io::SeekFrom) -> Result<u64, io::Error> {
        unimplemented!()
    }
}

impl FileLike for Stdout {
    fn metadata(&self) -> Result<Metadata, failure::Error> {
        unimplemented!()
    }

    fn set_file_len(&mut self, _len: usize) -> Result<(), failure::Error> {
        panic!("Cannot set length of stdout");
    }
}

impl io::Read for Stdout {
    fn read(&mut self, _buf: &mut [u8]) -> Result<usize, io::Error> {
        unimplemented!()
    }
}

impl io::Write for Stdout {
    fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        handle.write(buf)
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        handle.flush()
    }
}

impl io::Seek for Stdout {
    fn seek(&mut self, _pos: io::SeekFrom) -> Result<u64, io::Error> {
        unimplemented!()
    }
}

impl FileLike for Stderr {
    fn metadata(&self) -> Result<Metadata, failure::Error> {
        unimplemented!()
    }

    fn set_file_len(&mut self, _len: usize) -> Result<(), failure::Error> {
        panic!("Cannot set length of stderr");
    }
}

impl io::Read for Stderr {
    fn read(&mut self, _buf: &mut [u8]) -> Result<usize, io::Error> {
        unimplemented!()
    }
}

impl io::Write for Stderr {
    fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        let stderr = io::stderr();
        let mut handle = stderr.lock();
        handle.write(buf)
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        let stderr = io::stderr();
        let mut handle = stderr.lock();
        handle.flush()
    }
}

impl io::Seek for Stderr {
    fn seek(&mut self, _pos: io::SeekFrom) -> Result<u64, io::Error> {
        unimplemented!()
    }
}
