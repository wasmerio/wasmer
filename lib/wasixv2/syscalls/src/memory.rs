use std::{future::Future, pin::Pin};

use wasixv2_abi::{GuestLayout, GuestPtr, GuestPtrMut};

/// Boxed future without a `Send` bound; wasmer's async host calls are not `Send`.
pub type LocalFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// A guest address outside of mapped memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault;

/// The calling task's address space, read and written the way Linux's `copy_{from,to}_user` do.
pub trait UserMemory {
    fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>>;
    fn write<'a>(&'a self, addr: u32, data: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>>;
    /// Size of the address space in bytes.
    fn size(&self) -> LocalFuture<'_, u64>;
    /// Grows the address space by `pages` wasm pages of 64 KiB and returns its previous size in
    /// bytes, or `None` if it cannot grow that far.
    fn grow(&self, pages: u32) -> LocalFuture<'_, Option<u64>>;
}

/// Bytes of a wasm page, the unit `UserMemory::grow` counts in.
pub const WASM_PAGE_SIZE: u64 = 64 * 1024;

/// A C string longer than the limit the caller gave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CstrError {
    Fault,
    TooLong,
}

impl From<Fault> for CstrError {
    fn from(_: Fault) -> Self {
        Self::Fault
    }
}

/// Reads the value `ptr` points to.
pub async fn read_value<T: GuestLayout>(
    memory: &dyn UserMemory,
    ptr: GuestPtr<T>,
) -> Result<T, Fault> {
    let mut buf = vec![0; T::SIZE];
    memory.read(ptr.addr(), &mut buf).await?;
    T::read_from(&buf).ok_or(Fault)
}

/// Writes `value` where `ptr` points.
pub async fn write_value<T: GuestLayout>(
    memory: &dyn UserMemory,
    ptr: GuestPtrMut<T>,
    value: &T,
) -> Result<(), Fault> {
    let mut buf = vec![0; T::SIZE];
    value.write_to(&mut buf);
    memory.write(ptr.addr(), &buf).await
}

/// Reads the NUL-terminated string at `ptr`, without the NUL, failing with `TooLong` unless the
/// NUL is within `max` bytes, as Linux's `strncpy_from_user` does for `PATH_MAX`.
pub async fn read_cstr(
    memory: &dyn UserMemory,
    ptr: GuestPtr<u8>,
    max: usize,
) -> Result<Vec<u8>, CstrError> {
    // Aligned chunks never straddle the end of memory, which is a multiple of the chunk size.
    const CHUNK: u32 = 256;
    let mut out = Vec::new();
    let mut addr = ptr.addr();
    while out.len() < max {
        let len = CHUNK - addr % CHUNK;
        let mut chunk = vec![0; len as usize];
        memory.read(addr, &mut chunk).await?;
        if let Some(nul) = chunk.iter().position(|&b| b == 0) {
            out.extend_from_slice(&chunk[..nul]);
            return if out.len() < max {
                Ok(out)
            } else {
                Err(CstrError::TooLong)
            };
        }
        out.extend_from_slice(&chunk);
        addr = addr.checked_add(len).ok_or(Fault)?;
    }
    Err(CstrError::TooLong)
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
