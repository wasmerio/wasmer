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
    /// Whether the address space can be duplicated for a child that does not share it, as
    /// `clone` without `CLONE_VM` does (`dup_mm`). Wasm has no copy-on-write and keeps the call
    /// stack outside linear memory, so only an engine that can capture the caller's stack says
    /// yes: the asyncify build configuration, called from the guest's entry point.
    fn can_duplicate(&self) -> LocalFuture<'_, bool> {
        Box::pin(async { false })
    }

    /// The calling task's stack pointer as the call found it (`current_user_stack_pointer`):
    /// the guest's shadow stack pointer, which tells whether it runs on its alternate signal
    /// stack. 0 for memory no guest call runs on.
    fn stack_pointer(&self) -> LocalFuture<'_, u32> {
        Box::pin(async { 0 })
    }

    /// Replaces the `u32` at `addr`, which is 4-byte aligned, with `new` if it holds `expected`,
    /// and returns what it held (`futex_atomic_cmpxchg_inatomic`). The futex code relies on
    /// this to change a futex word under the feet of guest threads.
    ///
    /// Memory that other threads use at the same time must override this with an atomic
    /// compare-and-exchange. The default reads and then writes, which is only right when
    /// nothing else touches the memory meanwhile.
    fn compare_exchange_u32(
        &self,
        addr: u32,
        expected: u32,
        new: u32,
    ) -> LocalFuture<'_, Result<u32, Fault>> {
        Box::pin(async move {
            let mut word = [0; 4];
            self.read(addr, &mut word).await?;
            let current = u32::from_le_bytes(word);
            if current == expected {
                self.write(addr, &new.to_le_bytes()).await?;
            }
            Ok(current)
        })
    }
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
    let mut scratch = Scratch::new(T::SIZE);
    let buf = scratch.get();
    memory.read(ptr.addr(), buf).await?;
    T::read_from(buf).ok_or(Fault)
}

/// Writes `value` where `ptr` points.
pub async fn write_value<T: GuestLayout>(
    memory: &dyn UserMemory,
    ptr: GuestPtrMut<T>,
    value: &T,
) -> Result<(), Fault> {
    let mut scratch = Scratch::new(T::SIZE);
    let buf = scratch.get();
    value.write_to(buf);
    memory.write(ptr.addr(), buf).await
}

/// A buffer for one guest value: on the stack for the scalars and small structs nearly every
/// copy is about, on the heap for the few larger ones.
enum Scratch {
    Stack([u8; 128], usize),
    Heap(Vec<u8>),
}

impl Scratch {
    fn new(size: usize) -> Self {
        match size <= 128 {
            true => Self::Stack([0; 128], size),
            false => Self::Heap(vec![0; size]),
        }
    }

    fn get(&mut self) -> &mut [u8] {
        match self {
            Self::Stack(bytes, size) => &mut bytes[..*size],
            Self::Heap(bytes) => bytes,
        }
    }
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

    /// Copies `data` out to the start of the buffer. More than fits faults rather than write
    /// past the buffer.
    pub async fn copy_out(self, memory: &dyn UserMemory, data: &[u8]) -> Result<(), Fault> {
        if data.len() > self.len as usize {
            return Err(Fault);
        }
        memory.write(self.addr, data).await
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    struct Flat(RefCell<Vec<u8>>);

    impl UserMemory for Flat {
        fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
            let memory = self.0.borrow();
            let result = memory
                .get(addr as usize..addr as usize + buf.len())
                .map(|src| buf.copy_from_slice(src))
                .ok_or(Fault);
            Box::pin(async move { result })
        }

        fn write<'a>(&'a self, addr: u32, data: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
            let mut memory = self.0.borrow_mut();
            let result = memory
                .get_mut(addr as usize..addr as usize + data.len())
                .map(|dst| dst.copy_from_slice(data))
                .ok_or(Fault);
            Box::pin(async move { result })
        }

        fn size(&self) -> LocalFuture<'_, u64> {
            let size = self.0.borrow().len() as u64;
            Box::pin(async move { size })
        }

        fn grow(&self, _: u32) -> LocalFuture<'_, Option<u64>> {
            Box::pin(async { None })
        }
    }

    #[test]
    fn copy_out_faults_instead_of_writing_past_the_buffer() {
        let memory = Flat(RefCell::new(vec![0; 8]));
        let buf = UserSliceMut::new(0, 2);
        let copy = |data: &[u8]| {
            let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
            std::pin::pin!(buf.copy_out(&memory, data)).poll(&mut cx)
        };
        assert_eq!(copy(b"abc"), std::task::Poll::Ready(Err(Fault)));
        assert_eq!(*memory.0.borrow(), [0; 8]);
        assert_eq!(copy(b"ab"), std::task::Poll::Ready(Ok(())));
        assert_eq!(memory.0.borrow()[..3], *b"ab\0");
    }
}
