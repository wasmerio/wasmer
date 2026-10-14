use std::{future::Future, pin::Pin};

/// Boxed future without a `Send` bound; wasmer's async host calls are not `Send`.
pub type LocalFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// A guest address outside of mapped memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault;

/// The calling task's address space, read and written the way Linux's `copy_{from,to}_user` do.
pub trait UserMemory {
    fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>>;
    fn write<'a>(&'a self, addr: u32, data: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>>;
}

/// A guest buffer the kernel reads from. Nothing is copied until the kernel asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserSlice {
    pub addr: u32,
    pub len: u32,
}

impl UserSlice {
    pub fn new(addr: u32, len: u32) -> Self {
        Self { addr, len }
    }

    /// Copies at most `max` bytes in from the guest.
    pub async fn copy_in(self, memory: &dyn UserMemory, max: usize) -> Result<Vec<u8>, Fault> {
        let mut buf = vec![0; (self.len as usize).min(max)];
        memory.read(self.addr, &mut buf).await?;
        Ok(buf)
    }
}

/// A guest buffer the kernel writes into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserSliceMut {
    pub addr: u32,
    pub len: u32,
}

impl UserSliceMut {
    pub fn new(addr: u32, len: u32) -> Self {
        Self { addr, len }
    }

    /// Copies `data` out to the start of the buffer. `data` must fit in it.
    pub async fn copy_out(self, memory: &dyn UserMemory, data: &[u8]) -> Result<(), Fault> {
        debug_assert!(data.len() <= self.len as usize);
        memory.write(self.addr, data).await
    }
}
