//! Used for sharing references to the same file across multiple file systems,
//! effectively this is a symbolic link without all the complex path redirection

use crate::FileDescriptor;
use crate::{ClonableVirtualFile, VirtualFile};
use derivative::Derivative;
use std::{
    io::{self, *},
    sync::{Arc, Mutex},
};

#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct ArcFile {
    #[derivative(Debug = "ignore")]
    inner: Arc<Mutex<Box<dyn VirtualFile + Send + Sync + 'static>>>,
}

impl ArcFile {
    pub fn new(inner: Box<dyn VirtualFile + Send + Sync + 'static>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }
}

impl Seek for ArcFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let mut inner = self.inner.lock().unwrap();
        inner.seek(pos)
    }
}

impl Write for ArcFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut inner = self.inner.lock().unwrap();
        inner.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.flush()
    }
}

impl Read for ArcFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut inner = self.inner.lock().unwrap();
        inner.read(buf)
    }
}

impl VirtualFile for ArcFile {
    fn last_accessed(&self) -> u64 {
        let inner = self.inner.lock().unwrap();
        inner.last_accessed()
    }
    fn last_modified(&self) -> u64 {
        let inner = self.inner.lock().unwrap();
        inner.last_modified()
    }
    fn created_time(&self) -> u64 {
        let inner = self.inner.lock().unwrap();
        inner.created_time()
    }
    fn size(&self) -> u64 {
        let inner = self.inner.lock().unwrap();
        inner.size()
    }
    fn set_len(&mut self, new_size: u64) -> crate::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.set_len(new_size)
    }
    fn unlink(&mut self) -> crate::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.unlink()
    }
    fn bytes_available(&self) -> crate::Result<usize> {
        let inner = self.inner.lock().unwrap();
        inner.bytes_available()
    }
    fn bytes_available_read(&self) -> crate::Result<Option<usize>> {
        let inner = self.inner.lock().unwrap();
        inner.bytes_available_read()
    }
    fn bytes_available_write(&self) -> crate::Result<Option<usize>> {
        let inner = self.inner.lock().unwrap();
        inner.bytes_available_write()
    }
    fn get_fd(&self) -> Option<FileDescriptor> {
        let inner = self.inner.lock().unwrap();
        inner.get_fd()
    }
    fn is_open(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.is_open()
    }
    fn get_special_fd(&self) -> Option<u32> {
        let inner = self.inner.lock().unwrap();
        inner.get_special_fd()
    }
}

impl ClonableVirtualFile for ArcFile {}
