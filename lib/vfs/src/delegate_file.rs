use crate::FileDescriptor;
use crate::VirtualFile;
use derivative::Derivative;
use std::{
    io::{self, *},
    sync::{Arc, RwLock},
};

type DelegateSeekFn = Box<dyn Fn(SeekFrom) -> io::Result<u64> + Send + Sync>;
type DelegateWriteFn = Box<dyn Fn(&[u8]) -> io::Result<usize> + Send + Sync>;
type DelegateFlushFn = Box<dyn Fn() -> io::Result<()> + Send + Sync>;
type DelegateReadFn = Box<dyn Fn(&mut [u8]) -> io::Result<usize> + Send + Sync>;
type DelegateSizeFn = Box<dyn Fn() -> u64 + Send + Sync>;
type DelegateSetLenFn = Box<dyn Fn(u64) -> crate::Result<()> + Send + Sync>;
type DelegateUnlinkFn = Box<dyn Fn() -> crate::Result<()> + Send + Sync>;
type DelegateBytesAvailableFn = Box<dyn Fn() -> crate::Result<usize> + Send + Sync>;

#[derive(Default)]
pub struct DelegateFileInner {
    seek: Option<DelegateSeekFn>,
    write: Option<DelegateWriteFn>,
    flush: Option<DelegateFlushFn>,
    read: Option<DelegateReadFn>,
    size: Option<DelegateSizeFn>,
    set_len: Option<DelegateSetLenFn>,
    unlink: Option<DelegateUnlinkFn>,
    bytes_available: Option<DelegateBytesAvailableFn>,
}

/// Wrapper that forwards calls to `read`, `write`, etc.
/// to custom, user-defined functions - similar to `VirtualFile`
/// itself, except you don't have to create a new struct in order
/// to implement functions
#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct DelegateFile {
    #[derivative(Debug = "ignore")]
    inner: Arc<RwLock<DelegateFileInner>>,
}

impl DelegateFile {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_seek(
        &self,
        func: impl Fn(SeekFrom) -> io::Result<u64> + Send + Sync + 'static,
    ) -> &Self {
        let mut inner = self.inner.write().unwrap();
        inner.seek.replace(Box::new(func));
        self
    }

    pub fn with_write(
        &self,
        func: impl Fn(&[u8]) -> io::Result<usize> + Send + Sync + 'static,
    ) -> &Self {
        let mut inner = self.inner.write().unwrap();
        inner.write.replace(Box::new(func));
        self
    }

    pub fn with_flush(&self, func: impl Fn() -> io::Result<()> + Send + Sync + 'static) -> &Self {
        let mut inner = self.inner.write().unwrap();
        inner.flush.replace(Box::new(func));
        self
    }

    pub fn with_read(
        &self,
        func: impl Fn(&mut [u8]) -> io::Result<usize> + Send + Sync + 'static,
    ) -> &Self {
        let mut inner = self.inner.write().unwrap();
        inner.read.replace(Box::new(func));
        self
    }

    pub fn with_size(&self, func: impl Fn() -> u64 + Send + Sync + 'static) -> &Self {
        let mut inner = self.inner.write().unwrap();
        inner.size.replace(Box::new(func));
        self
    }

    pub fn with_set_len(
        &self,
        func: impl Fn(u64) -> crate::Result<()> + Send + Sync + 'static,
    ) -> &Self {
        let mut inner = self.inner.write().unwrap();
        inner.set_len.replace(Box::new(func));
        self
    }

    pub fn with_unlink(
        &self,
        func: impl Fn() -> crate::Result<()> + Send + Sync + 'static,
    ) -> &Self {
        let mut inner = self.inner.write().unwrap();
        inner.unlink.replace(Box::new(func));
        self
    }

    pub fn with_bytes_available(
        &self,
        func: impl Fn() -> crate::Result<usize> + Send + Sync + 'static,
    ) -> &Self {
        let mut inner = self.inner.write().unwrap();
        inner.bytes_available.replace(Box::new(func));
        self
    }
}

impl Default for DelegateFile {
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new(DelegateFileInner::default())),
        }
    }
}

impl Seek for DelegateFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let inner = self.inner.read().unwrap();
        inner.seek.as_ref().map(|seek| seek(pos)).unwrap_or(Ok(0))
    }
}
impl Write for DelegateFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let inner = self.inner.read().unwrap();
        inner
            .write
            .as_ref()
            .map(|write| write(buf))
            .unwrap_or(Ok(buf.len()))
    }
    fn flush(&mut self) -> io::Result<()> {
        let inner = self.inner.read().unwrap();
        inner.flush.as_ref().map(|flush| flush()).unwrap_or(Ok(()))
    }
}

impl Read for DelegateFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let inner = self.inner.read().unwrap();
        inner.read.as_ref().map(|read| read(buf)).unwrap_or(Ok(0))
    }
}

impl VirtualFile for DelegateFile {
    fn last_accessed(&self) -> u64 {
        0
    }
    fn last_modified(&self) -> u64 {
        0
    }
    fn created_time(&self) -> u64 {
        0
    }
    fn size(&self) -> u64 {
        let inner = self.inner.read().unwrap();
        inner.size.as_ref().map(|size| size()).unwrap_or(0)
    }
    fn set_len(&mut self, new_size: u64) -> crate::Result<()> {
        let inner = self.inner.read().unwrap();
        inner
            .set_len
            .as_ref()
            .map(|set_len| set_len(new_size))
            .unwrap_or(Ok(()))
    }
    fn unlink(&mut self) -> crate::Result<()> {
        let inner = self.inner.read().unwrap();
        inner
            .unlink
            .as_ref()
            .map(|unlink| unlink())
            .unwrap_or(Ok(()))
    }
    fn bytes_available(&self) -> crate::Result<usize> {
        let inner = self.inner.read().unwrap();
        inner
            .bytes_available
            .as_ref()
            .map(|bytes_available| bytes_available())
            .unwrap_or(Ok(0))
    }
    fn get_fd(&self) -> Option<FileDescriptor> {
        None
    }
}

#[test]
fn test_delegate_file() {
    let mut custom_write_buf = vec![0; 17];
    let mut file = DelegateFile::new();

    file.with_write(|_| Ok(384));
    file.with_read(|_| Ok(986));
    file.with_seek(|_| Ok(996));

    assert_eq!(file.read(custom_write_buf.as_mut_slice()).unwrap(), 986);
    assert_eq!(file.seek(SeekFrom::Start(0)).unwrap(), 996);
    assert_eq!(file.write(b"hello").unwrap(), 384);
}
