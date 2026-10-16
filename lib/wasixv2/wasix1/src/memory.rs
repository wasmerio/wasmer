//! The address space a v1 shim hands the kernel: the guest's linear memory, plus a scratch
//! window for values the shim must translate between the v1 and the v2 layouts.
//!
//! v2 syscalls take guest addresses. When a v1 struct has a different layout, the shim builds
//! the v2 value in scratch, passes its scratch address, and reads the result back from there.
//! Buffers and iovec arrays, whose layout is the same, go to the kernel untranslated, so bulk
//! data is still copied once.

use std::cell::RefCell;

use wasixv2_abi::{Cstr, CstrArray, GuestLayout, GuestPtr, GuestPtrMut};
use wasixv2_exec::GuestMemory;
use wasixv2_syscalls::{
    Errno, Fault, LocalFuture, SysResult, UserMemory, UserSlice, UserSliceMut, WASM_PAGE_SIZE,
};

/// Start of the scratch window: the top 16 MiB of the 32-bit address space, which the guest's
/// memory never reaches (see [`CompatMemory::grow`]).
pub const SCRATCH_BASE: u32 = 0xFF00_0000;
/// Size of the scratch window.
pub const SCRATCH_SIZE: u32 = 0x0100_0000;

/// The calling instance's memory below [`SCRATCH_BASE`], a per-call scratch arena above it.
///
/// The arena lives as long as the `CompatMemory`, one import call: allocations are never
/// freed individually.
pub struct CompatMemory<'a, S: 'static> {
    guest: GuestMemory<'a, S>,
    scratch: RefCell<Vec<u8>>,
}

impl<'a, S: 'static> CompatMemory<'a, S> {
    pub fn new(guest: GuestMemory<'a, S>) -> Self {
        Self {
            guest,
            scratch: RefCell::new(Vec::new()),
        }
    }

    /// The guest's own memory, for accesses that must not reach scratch.
    pub fn guest(&self) -> &GuestMemory<'a, S> {
        &self.guest
    }

    /// Reserves `len` zeroed bytes aligned to `align` and returns their address. Fails with
    /// `ENOMEM` when the window is full.
    pub fn alloc_raw(&self, len: u32, align: u32) -> SysResult<u32> {
        let mut scratch = self.scratch.borrow_mut();
        let align = align.max(1) as usize;
        let start = scratch.len().next_multiple_of(align);
        let end = start
            .checked_add(len as usize)
            .filter(|&end| end <= SCRATCH_SIZE as usize)
            .ok_or(Errno::ENOMEM)?;
        scratch.resize(end, 0);
        Ok(SCRATCH_BASE + start as u32)
    }

    /// A copy of `value` in scratch.
    pub fn alloc<T: GuestLayout>(&self, value: &T) -> SysResult<GuestPtr<T>> {
        let addr = self.alloc_raw(T::SIZE as u32, T::ALIGN as u32)?;
        self.with_scratch(addr, T::SIZE, |slot| value.write_to(slot));
        Ok(GuestPtr::new(addr))
    }

    /// Zeroed room for a `T` the kernel writes; read it back with [`CompatMemory::take`].
    pub fn alloc_mut<T: GuestLayout>(&self) -> SysResult<GuestPtrMut<T>> {
        self.alloc_raw(T::SIZE as u32, T::ALIGN as u32)
            .map(GuestPtrMut::new)
    }

    /// A copy of `bytes` in scratch.
    pub fn alloc_bytes(&self, bytes: &[u8]) -> SysResult<UserSlice> {
        let len = u32::try_from(bytes.len()).map_err(|_| Errno::ENOMEM)?;
        let addr = self.alloc_raw(len, 1)?;
        self.with_scratch(addr, bytes.len(), |slot| slot.copy_from_slice(bytes));
        Ok(UserSlice::new(addr, len))
    }

    /// `len` zeroed bytes in scratch, aligned to 16 as the kernel's structured buffers want.
    pub fn alloc_zeroed(&self, len: u32) -> SysResult<UserSliceMut> {
        self.alloc_raw(len, 16)
            .map(|addr| UserSliceMut::new(addr, len))
    }

    /// `s` with a NUL appended, in scratch.
    pub fn alloc_cstr(&self, s: &[u8]) -> SysResult<Cstr> {
        let mut bytes = Vec::with_capacity(s.len() + 1);
        bytes.extend_from_slice(s);
        bytes.push(0);
        self.alloc_bytes(&bytes)
            .map(|slice| GuestPtr::new(slice.addr))
    }

    /// A NULL-terminated array of C strings in scratch, as `execve` takes `argv`.
    pub fn alloc_cstr_array(&self, items: &[&[u8]]) -> SysResult<CstrArray> {
        let pointers = items
            .iter()
            .map(|item| self.alloc_cstr(item))
            .collect::<SysResult<Vec<_>>>()?;
        let addr = self.alloc_raw(4 * (pointers.len() as u32 + 1), 4)?;
        self.with_scratch(addr, 4 * (pointers.len() + 1), |slot| {
            for (chunk, pointer) in slot.chunks_exact_mut(4).zip(&pointers) {
                chunk.copy_from_slice(&pointer.addr().to_le_bytes());
            }
        });
        Ok(GuestPtr::new(addr))
    }

    /// The `T` the kernel left at `ptr`, a scratch address from [`CompatMemory::alloc_mut`].
    pub fn take<T: GuestLayout>(&self, ptr: GuestPtrMut<T>) -> T {
        let mut out = T::ZERO;
        self.with_scratch(ptr.addr(), T::SIZE, |slot| {
            out = T::read_from(slot).expect("scratch slot holds a whole value");
        });
        out
    }

    /// The first `len` bytes of the scratch buffer `slice`.
    pub fn take_bytes(&self, slice: UserSliceMut, len: usize) -> Vec<u8> {
        let mut out = Vec::new();
        self.with_scratch(slice.addr, len.min(slice.len as usize), |slot| {
            out = slot.to_vec();
        });
        out
    }

    /// Runs `f` on scratch bytes `addr..addr + len`, which an allocation returned.
    fn with_scratch(&self, addr: u32, len: usize, f: impl FnOnce(&mut [u8])) {
        let start = (addr - SCRATCH_BASE) as usize;
        f(&mut self.scratch.borrow_mut()[start..start + len]);
    }

    /// The window offsets `addr..addr + len`, or `None` if `addr` is below the window. The
    /// whole window reads as zeroes where nothing was allocated, as an anonymous mapping
    /// would, since the kernel reads strings in aligned chunks; a range past its end faults.
    fn scratch_range(addr: u32, len: usize) -> Option<Result<(usize, usize), Fault>> {
        if addr < SCRATCH_BASE {
            return None;
        }
        let start = (addr - SCRATCH_BASE) as usize;
        Some(
            start
                .checked_add(len)
                .filter(|&end| end <= SCRATCH_SIZE as usize)
                .map(|end| (start, end))
                .ok_or(Fault),
        )
    }
}

/// A guest range that ends inside the scratch window but starts below it.
fn straddles(addr: u32, len: usize) -> bool {
    addr < SCRATCH_BASE && u64::from(addr) + len as u64 > u64::from(SCRATCH_BASE)
}

impl<S: 'static> UserMemory for CompatMemory<'_, S> {
    fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        match Self::scratch_range(addr, buf.len()) {
            Some(range) => {
                let result = range.map(|(start, end)| {
                    let scratch = self.scratch.borrow();
                    let held = end.min(scratch.len()).max(start);
                    let (from_scratch, zeroes) = buf.split_at_mut(held - start);
                    from_scratch.copy_from_slice(&scratch[start..held]);
                    zeroes.fill(0);
                });
                Box::pin(async move { result })
            }
            None if straddles(addr, buf.len()) => Box::pin(async { Err(Fault) }),
            None => self.guest.read(addr, buf),
        }
    }

    fn write<'a>(&'a self, addr: u32, data: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        match Self::scratch_range(addr, data.len()) {
            Some(range) => {
                let result = range.map(|(start, end)| {
                    let mut scratch = self.scratch.borrow_mut();
                    if scratch.len() < end {
                        scratch.resize(end, 0);
                    }
                    scratch[start..end].copy_from_slice(data);
                });
                Box::pin(async move { result })
            }
            None if straddles(addr, data.len()) => Box::pin(async { Err(Fault) }),
            None => self.guest.write(addr, data),
        }
    }

    fn size(&self) -> LocalFuture<'_, u64> {
        self.guest.size()
    }

    /// Grows the guest's memory, but never into the scratch window.
    fn grow(&self, pages: u32) -> LocalFuture<'_, Option<u64>> {
        Box::pin(async move {
            let size = self.guest.size().await;
            if size + u64::from(pages) * WASM_PAGE_SIZE > u64::from(SCRATCH_BASE) {
                return None;
            }
            self.guest.grow(pages).await
        })
    }
}
