//! Guest memory as the host reaches it, and the low guard.
//!
//! The low guard makes the first [`LOW_GUARD`] bytes of an instance's linear memory read-only,
//! where Linux keeps the pages below `mmap_min_addr` unmapped (`mm/Kconfig`,
//! `DEFAULT_MMAP_MIN_ADDR`; `security/min_addr.c`): a store through a null pointer, or a small
//! offset from one, faults and kills the process with `SIGSEGV`, where a wasm guest would
//! otherwise overwrite whatever lives at address 0. It needs a module that keeps nothing there,
//! which lld's `--stack-first` layout does: the main stack grows down towards 0 from
//! `__stack_pointer`, with data above it, so only a stack about to overflow reaches the guard.
//!
//! Loads stay allowed, and read zeros, because clang's wasm32 backend passes a null va_list
//! buffer to a variadic call without variadic arguments (`WebAssemblyISelLowering.cpp`,
//! `LowerCall`), and musl reads `va_arg` regardless (`fcntl(fd, F_GETFD)`, `__clone` from
//! `posix_spawn`, `mremap`, `ioctl`, `semctl`, `prctl`, `syscall`). See DESIGN.md, "Guard page
//! at 0", for the follow-up that makes the guard `PROT_NONE`.
//!
//! Every host write to guest memory goes through [`write`], which faults inside the guard
//! instead of touching the protected pages, which would kill the host; [`read`] reads there as
//! the guest does.
//!
//! A range of memory can show a shared object's pages instead of its own ([`map_shared`]): the
//! host maps the object over the range, inside the memory's reservation, which never moves.
//! Every process that maps the object then sees the same bytes at once. The range is never
//! placed over the low guard, which `MAP_FIXED` would make writable again. A view that may
//! never be written (a `SHM_RDONLY` attachment, a shared mapping of a file open only for
//! reading) is mapped read-only, the way the low guard is protected: a guest store there faults
//! with `SIGSEGV`, and [`ReadOnly`] keeps [`write`] out of it. Every other range stays
//! read-write whatever the guest asked for, as wasm enforces no page protection of its own.

use std::{
    ops::Range,
    sync::{PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard},
};

#[cfg(all(unix, feature = "sys"))]
use wasixv2_pages::SharedPages;
#[cfg(all(unix, feature = "sys"))]
use wasixv2_syscalls::Errno;
use wasixv2_syscalls::{Fault, LocalFuture, UserMemory, copied};
use wasmer::{
    AsStoreMut, AsStoreRef, AsyncFunctionEnvHandle, AsyncFunctionEnvMut, Instance, Memory,
};

use crate::{
    ExecEnv,
    asyncify::{Access, Outside},
};

/// Bytes at the bottom of linear memory the low guard covers: `mmap_min_addr`'s 65536, the value
/// x86 and arm64 distributions configure.
pub(crate) const LOW_GUARD: u32 = 65536;

/// The smallest initial stack pointer that gets the guard. The guard takes the bottom
/// [`LOW_GUARD`] bytes of the main stack, so a module with a smaller stack would run into it
/// long before its stack is used up.
const MIN_GUARDED_STACK: u32 = 2 * LOW_GUARD;

/// Whether a module whose `__stack_pointer` starts at `stack_pointer` and whose data ends at
/// `data_end` keeps nothing in the low guard: lld's stack-first layout puts the stack at 0 and
/// the data above it (`stack_pointer <= data_end`); data-first puts the stack above the data.
pub(crate) fn guardable_layout(stack_pointer: u32, data_end: u32) -> bool {
    stack_pointer <= data_end && stack_pointer >= MIN_GUARDED_STACK
}

/// Whether `instance`, before any of its code ran, has a layout that keeps the low guard free,
/// from the `__stack_pointer` and `__data_end` it exports.
pub(crate) fn guardable(instance: &Instance, store: &mut impl AsStoreMut) -> bool {
    let mut value = |name: &str| {
        let global = instance.exports.get_global(name).ok()?;
        global.get(&mut *store).i32().map(|value| value as u32)
    };
    match (value("__stack_pointer"), value("__data_end")) {
        (Some(stack_pointer), Some(data_end)) => guardable_layout(stack_pointer, data_end),
        _ => false,
    }
}

/// Makes the low guard of `memory` read-only, and returns its size: [`LOW_GUARD`], or 0 where
/// the host cannot protect pages. Once suffices for the life of the memory: memories are
/// reserved to their maximum (`MemoryStyle::Static`), so growing them never moves their base.
#[cfg(all(unix, feature = "sys"))]
pub(crate) fn protect_low(memory: &Memory, store: &impl AsStoreRef) -> u32 {
    let view = memory.view(store);
    if view.data_size() < u64::from(LOW_GUARD) {
        return 0;
    }
    // SAFETY: the range is the start of the memory's own mapping, which is page aligned and at
    // least that large; the host writes guest memory only through `write`, which faults there.
    let ret =
        unsafe { libc::mprotect(view.data_ptr().cast(), LOW_GUARD as usize, libc::PROT_READ) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        tracing::warn!(%err, "cannot protect the low guard of a guest's memory");
        return 0;
    }
    LOW_GUARD
}

/// Hosts without page protection go without the guard (Windows needs `VirtualProtect`).
#[cfg(not(all(unix, feature = "sys")))]
pub(crate) fn protect_low(_memory: &Memory, _store: &impl AsStoreRef) -> u32 {
    0
}

/// `len` bytes at `addr` reach into a low guard of `low_guard` bytes. An empty access touches
/// nothing, as `copy_from_user` of 0 bytes never faults.
fn in_guard(low_guard: u32, addr: u32, len: usize) -> bool {
    len > 0 && addr < low_guard
}

/// The ranges of one linear memory that show a shared object read-only ([`map_shared`] without
/// `writable`), where the host has no write access and must not write either, as in the low
/// guard. Shared by the instances of all threads of a process, as they share the memory.
///
/// A host write holds the lock while it copies, and placing or removing a view holds it
/// exclusively, so a range never loses write access under a copy into it, which would kill the
/// host, however the guest's threads race.
#[derive(Default)]
pub(crate) struct ReadOnly(RwLock<Vec<Range<u64>>>);

impl ReadOnly {
    fn ranges(&self) -> RwLockReadGuard<'_, Vec<Range<u64>>> {
        self.0.read().unwrap_or_else(PoisonError::into_inner)
    }

    #[cfg(all(unix, feature = "sys"))]
    fn ranges_mut(&self) -> RwLockWriteGuard<'_, Vec<Range<u64>>> {
        self.0.write().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Whether `len` bytes at `addr` reach into one of `ranges`. An empty access touches nothing.
fn in_read_only(ranges: &[Range<u64>], addr: u32, len: usize) -> bool {
    let (start, end) = (u64::from(addr), u64::from(addr) + len as u64);
    len > 0
        && ranges
            .iter()
            .any(|range| range.start < end && start < range.end)
}

/// Records `range` as read-only or not, in place of what `ranges` said of it.
#[cfg(all(unix, feature = "sys"))]
fn set_read_only(ranges: &mut Vec<Range<u64>>, range: Range<u64>, read_only: bool) {
    let mut kept = Vec::with_capacity(ranges.len() + 2);
    for old in ranges.drain(..) {
        if old.end <= range.start || range.end <= old.start {
            kept.push(old);
            continue;
        }
        if old.start < range.start {
            kept.push(old.start..range.start);
        }
        if range.end < old.end {
            kept.push(range.end..old.end);
        }
    }
    if read_only {
        kept.push(range);
    }
    *ranges = kept;
}

/// Copies `addr..` of `memory` into `buf`; a fault outside the memory. The low guard is
/// readable, as it is to the guest.
pub(crate) fn read(
    memory: &Memory,
    store: &impl AsStoreRef,
    addr: u32,
    buf: &mut [u8],
) -> Result<(), Fault> {
    memory.view(store).read(addr.into(), buf).map_err(|_| Fault)
}

/// Copies `bytes` to `addr..` of `memory`, whose low guard is `low_guard` bytes and whose
/// read-only views are `read_only`; a fault outside the memory, inside the guard or inside a
/// read-only view.
pub(crate) fn write(
    memory: &Memory,
    low_guard: u32,
    read_only: &ReadOnly,
    store: &impl AsStoreRef,
    addr: u32,
    bytes: &[u8],
) -> Result<(), Fault> {
    if in_guard(low_guard, addr, bytes.len()) {
        return Err(Fault);
    }
    let read_only = read_only.ranges();
    if in_read_only(&read_only, addr, bytes.len()) {
        return Err(Fault);
    }
    memory
        .view(store)
        .write(addr.into(), bytes)
        .map_err(|_| Fault)
}

/// Makes `[addr, addr + len)` of `memory`, whose low guard is `low_guard` bytes and whose
/// read-only views are `read_only`, show `len` bytes of `pages` from `offset`, read-only unless
/// `writable` ([`UserMemory::map_shared`]). `EINVAL` for a range outside the memory, inside the
/// guard or not page aligned, or past the end of the object.
#[cfg(all(unix, feature = "sys"))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn map_shared(
    memory: &Memory,
    low_guard: u32,
    read_only: &ReadOnly,
    store: &impl AsStoreRef,
    (addr, len): (u32, u64),
    pages: &dyn SharedPages,
    offset: u64,
    writable: bool,
) -> Result<(), Errno> {
    let ptr = shared_range(memory, low_guard, store, addr, len)?;
    let mut ranges = read_only.ranges_mut();
    // SAFETY: the range lies inside the memory's own mapping, which never moves (memories are
    // reserved to their maximum) and which the host reaches only through views, by copies, and
    // writes to only under `read_only`'s lock, held here.
    let mapped = unsafe { wasixv2_pages::map_at(ptr, len as usize, pages, offset, writable) };
    // A failure may leave the range as it was, or writable private pages: either way the
    // record stays, which at worst refuses a host write the range would take.
    if mapped.is_ok() {
        let range = u64::from(addr)..u64::from(addr) + len;
        set_read_only(&mut ranges, range, !writable);
    }
    mapped.map_err(|err| host_errno(&err))
}

/// Gives `[addr, addr + len)` of `memory` its own zeroed pages again
/// ([`UserMemory::unmap_shared`]).
#[cfg(all(unix, feature = "sys"))]
pub(crate) fn unmap_shared(
    memory: &Memory,
    low_guard: u32,
    read_only: &ReadOnly,
    store: &impl AsStoreRef,
    addr: u32,
    len: u64,
) -> Result<(), Errno> {
    let ptr = shared_range(memory, low_guard, store, addr, len)?;
    let mut ranges = read_only.ranges_mut();
    // SAFETY: as in `map_shared`.
    let unmapped = unsafe { wasixv2_pages::map_private_at(ptr, len as usize) };
    if unmapped.is_ok() {
        set_read_only(&mut ranges, u64::from(addr)..u64::from(addr) + len, false);
    }
    unmapped.map_err(|err| host_errno(&err))
}

/// The host address of `[addr, addr + len)` of `memory`, a range a shared object may cover:
/// inside the memory and above its low guard.
#[cfg(all(unix, feature = "sys"))]
fn shared_range(
    memory: &Memory,
    low_guard: u32,
    store: &impl AsStoreRef,
    addr: u32,
    len: u64,
) -> Result<*mut u8, Errno> {
    let view = memory.view(store);
    let inside = u64::from(addr)
        .checked_add(len)
        .is_some_and(|end| end <= view.data_size());
    if !inside || len == 0 || addr < low_guard {
        return Err(Errno::EINVAL);
    }
    // SAFETY: in bounds, checked above.
    Ok(unsafe { view.data_ptr().add(addr as usize) })
}

/// The errno for a host error placing pages: `EOPNOTSUPP` for an object the host cannot map,
/// `EINVAL` for a range it refused, `ENOMEM` for anything else (out of mappings or memory).
#[cfg(all(unix, feature = "sys"))]
fn host_errno(err: &std::io::Error) -> Errno {
    match (err.kind(), err.raw_os_error()) {
        (std::io::ErrorKind::Unsupported, _) => Errno::EOPNOTSUPP,
        (_, Some(libc::EINVAL)) => Errno::EINVAL,
        _ => Errno::ENOMEM,
    }
}

impl<S> ExecEnv<S> {
    /// [`map_shared`] of the instance's memory.
    #[cfg(all(unix, feature = "sys"))]
    pub(crate) fn map_shared_memory(
        &self,
        store: &impl AsStoreRef,
        (addr, len, writable): (u32, u64, bool),
        pages: &dyn SharedPages,
        offset: u64,
    ) -> Result<(), Errno> {
        let memory = self.memory.as_ref().ok_or(Errno::EFAULT)?;
        let (guard, read_only) = (self.low_guard, &*self.read_only);
        map_shared(
            memory,
            guard,
            read_only,
            store,
            (addr, len),
            pages,
            offset,
            writable,
        )
    }

    /// [`unmap_shared`] of the instance's memory.
    #[cfg(all(unix, feature = "sys"))]
    pub(crate) fn unmap_shared_memory(
        &self,
        store: &impl AsStoreRef,
        addr: u32,
        len: u64,
    ) -> Result<(), Errno> {
        let memory = self.memory.as_ref().ok_or(Errno::EFAULT)?;
        unmap_shared(memory, self.low_guard, &self.read_only, store, addr, len)
    }

    /// [`read`] of the instance's memory.
    pub(crate) fn read_memory(
        &self,
        store: &impl AsStoreRef,
        addr: u32,
        buf: &mut [u8],
    ) -> Result<(), Fault> {
        let memory = self.memory.as_ref().ok_or(Fault)?;
        read(memory, store, addr, buf)
    }

    /// [`write`] to the instance's memory.
    pub(crate) fn write_memory(
        &self,
        store: &impl AsStoreRef,
        addr: u32,
        bytes: &[u8],
    ) -> Result<(), Fault> {
        let memory = self.memory.as_ref().ok_or(Fault)?;
        write(memory, self.low_guard, &self.read_only, store, addr, bytes)
    }
}

/// The calling instance's linear memory, reached through its async function environment.
pub struct GuestMemory<'a, S: 'static> {
    pub(crate) env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
}

impl<'a, S: 'static> GuestMemory<'a, S> {
    pub fn new(env: &'a AsyncFunctionEnvMut<ExecEnv<S>>) -> Self {
        Self { env }
    }
}

/// Copies `addr..` of the instance's linear memory into `buf`.
fn read_at<S: 'static>(
    env: &AsyncFunctionEnvHandle<ExecEnv<S>>,
    addr: u32,
    buf: &mut [u8],
) -> Result<(), Fault> {
    let (data, store) = env.data_and_store();
    data.read_memory(&store, addr, buf)
}

/// Copies `bytes` to `addr..` of the instance's linear memory.
fn write_at<S: 'static>(
    env: &AsyncFunctionEnvHandle<ExecEnv<S>>,
    addr: u32,
    bytes: &[u8],
) -> Result<(), Fault> {
    let (data, store) = env.data_and_store();
    data.write_memory(&store, addr, bytes)
}

/// Copies happen at once when the store is free, as it is while the guest waits for its own
/// import, so they allocate nothing ([`copied`]); they wait for the store otherwise.
impl<S: 'static> UserMemory for GuestMemory<'_, S> {
    fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        match self.env.try_read() {
            Some(env) => copied(read_at(&env, addr, buf)),
            None => Box::pin(async move { read_at(&self.env.read().await, addr, buf) }),
        }
    }

    fn write<'a>(&'a self, addr: u32, bytes: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        match self.env.try_read() {
            Some(env) => copied(write_at(&env, addr, bytes)),
            None => Box::pin(async move { write_at(&self.env.read().await, addr, bytes) }),
        }
    }

    fn size(&self) -> LocalFuture<'_, u64> {
        Box::pin(async move {
            let mut env = self.env.write().await;
            let (data, store) = env.data_and_store_mut();
            data.memory
                .as_ref()
                .map_or(0, |memory| memory.view(&*store).data_size())
        })
    }

    fn can_duplicate(&self) -> LocalFuture<'_, bool> {
        Box::pin(crate::asyncify::whole_stack(self.env))
    }

    fn stack_pointer(&self) -> LocalFuture<'_, u32> {
        Box::pin(async move {
            let mut env = self.env.write().await;
            let (data, mut store) = env.data_and_store_mut();
            let global = data.stack_pointer.as_ref();
            global.map_or(0, |global| global.get(&mut store).i32().unwrap_or(0) as u32)
        })
    }

    fn low_guard(&self) -> LocalFuture<'_, u32> {
        Box::pin(async move { self.env.read().await.data().low_guard })
    }

    /// An atomic compare-and-exchange on the memory itself, so guest threads running at the same
    /// time on other host threads see the futex word change as one step.
    #[cfg(feature = "sys")]
    fn compare_exchange_u32(
        &self,
        addr: u32,
        expected: u32,
        new: u32,
    ) -> LocalFuture<'_, Result<u32, Fault>> {
        use std::sync::atomic::{AtomicU32, Ordering};
        Box::pin(async move {
            let mut env = self.env.write().await;
            let (data, store) = env.data_and_store_mut();
            let memory = data.memory.as_ref().ok_or(Fault)?;
            let view = memory.view(&*store);
            let outside = u64::from(addr) + 4 > view.data_size();
            let read_only = data.read_only.ranges();
            let protected = in_guard(data.low_guard, addr, 4) || in_read_only(&read_only, addr, 4);
            if !addr.is_multiple_of(4) || outside || protected {
                return Err(Fault);
            }
            // SAFETY: the word is in bounds, outside the read-only low guard and views (a
            // compare-exchange may write), which stay so while `read_only` is held, and
            // aligned, and linear memory stays mapped while the store is borrowed; guest
            // threads only race with this through atomics.
            let word = unsafe { AtomicU32::from_ptr(view.data_ptr().add(addr as usize).cast()) };
            let current = word.compare_exchange(expected, new, Ordering::SeqCst, Ordering::SeqCst);
            Ok(current.unwrap_or_else(|actual| actual))
        })
    }

    fn grow(&self, pages: u32) -> LocalFuture<'_, Option<u64>> {
        Box::pin(async move {
            let mut env = self.env.write().await;
            let (data, store) = env.data_and_store_mut();
            let previous = data.memory.as_ref()?.grow(store, pages).ok()?;
            Some(previous.bytes().0 as u64)
        })
    }

    #[cfg(all(unix, feature = "sys"))]
    fn map_shared<'a>(
        &'a self,
        addr: u32,
        len: u64,
        pages: &'a dyn SharedPages,
        offset: u64,
        writable: bool,
    ) -> LocalFuture<'a, Result<(), Errno>> {
        Box::pin(async move {
            let mut env = self.env.write().await;
            let (data, store) = env.data_and_store_mut();
            data.map_shared_memory(&*store, (addr, len, writable), pages, offset)
        })
    }

    #[cfg(all(unix, feature = "sys"))]
    fn unmap_shared(&self, addr: u32, len: u64) -> LocalFuture<'_, Result<(), Errno>> {
        Box::pin(async move {
            let mut env = self.env.write().await;
            let (data, store) = env.data_and_store_mut();
            data.unmap_shared_memory(&*store, addr, len)
        })
    }
}

/// The linear memory of an instance seen from outside any guest call, for the calls the
/// execution layer makes on the guest's behalf, such as the exit when an entry point returns.
pub(crate) struct OutsideMemory<'a, 'b, S: 'static> {
    pub(crate) cx: &'a Outside<'b, S>,
}

impl<S: Send + Sync + 'static> UserMemory for OutsideMemory<'_, '_, S> {
    fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        Box::pin(
            self.cx
                .with(move |data, store| data.read_memory(&*store, addr, buf)),
        )
    }

    fn write<'a>(&'a self, addr: u32, bytes: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        Box::pin(
            self.cx
                .with(move |data, store| data.write_memory(&*store, addr, bytes)),
        )
    }

    fn size(&self) -> LocalFuture<'_, u64> {
        Box::pin(self.cx.with(|data, store| {
            data.memory
                .as_ref()
                .map_or(0, |memory| memory.view(&*store).data_size())
        }))
    }

    fn low_guard(&self) -> LocalFuture<'_, u32> {
        Box::pin(self.cx.with(|data, _| data.low_guard))
    }

    fn grow(&self, pages: u32) -> LocalFuture<'_, Option<u64>> {
        Box::pin(self.cx.with(move |data, store| {
            let previous = data.memory.as_ref()?.grow(store, pages).ok()?;
            Some(previous.bytes().0 as u64)
        }))
    }

    #[cfg(all(unix, feature = "sys"))]
    fn map_shared<'a>(
        &'a self,
        addr: u32,
        len: u64,
        pages: &'a dyn SharedPages,
        offset: u64,
        writable: bool,
    ) -> LocalFuture<'a, Result<(), Errno>> {
        Box::pin(self.cx.with(move |data, store| {
            data.map_shared_memory(&*store, (addr, len, writable), pages, offset)
        }))
    }

    #[cfg(all(unix, feature = "sys"))]
    fn unmap_shared(&self, addr: u32, len: u64) -> LocalFuture<'_, Result<(), Errno>> {
        Box::pin(
            self.cx
                .with(move |data, store| data.unmap_shared_memory(&*store, addr, len)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stack_first_layout_gets_the_guard() {
        // wasixcc's default: an 8 MiB stack at 0, then data.
        assert!(guardable_layout(8 << 20, 8_410_324));
        // Rust's 1 MiB stack, as long as it is first.
        assert!(guardable_layout(1 << 20, 1_066_864));
        assert!(guardable_layout(2 * LOW_GUARD, 2 * LOW_GUARD));
    }

    #[test]
    fn a_data_first_layout_does_not() {
        // lld's data-first layout (`--no-stack-first`, older lld): data from 1024, the stack
        // above it.
        assert!(!guardable_layout(73_536, 8_000));
        assert!(!guardable_layout(19_565_952, 11_177_340));
    }

    #[test]
    fn a_stack_too_small_to_spare_the_guard_does_not() {
        assert!(!guardable_layout(2 * LOW_GUARD - 16, 2 * LOW_GUARD));
        assert!(!guardable_layout(LOW_GUARD, 1 << 20));
    }

    #[cfg(all(
        feature = "sys",
        any(target_os = "linux", target_os = "android", target_os = "macos")
    ))]
    #[test]
    fn two_memories_mapping_one_object_share_its_pages() {
        use wasixv2_pages::{Memfd, SharedPages};
        use wasmer::{MemoryType, Store};

        const PAGE: u64 = 65536;
        let mut store = Store::default();
        let mut memory = || Memory::new(&mut store, MemoryType::new(4, None, true)).unwrap();
        let (a, b) = (memory(), memory());
        let pages = Memfd::new(2 * PAGE).unwrap();
        a.view(&store).write(2 * PAGE, b"private").unwrap();
        let (ra, rb) = (ReadOnly::default(), ReadOnly::default());
        let range = (2 * PAGE as u32, 2 * PAGE);
        map_shared(&a, LOW_GUARD, &ra, &store, range, &pages, 0, true).unwrap();
        let range = (PAGE as u32, PAGE);
        map_shared(&b, LOW_GUARD, &rb, &store, range, &pages, PAGE, true).unwrap();
        // The object's zeros replaced the private bytes.
        assert_eq!(a.view(&store).read_u8(2 * PAGE).unwrap(), 0);
        write(&a, LOW_GUARD, &ra, &store, 3 * PAGE as u32 + 8, b"seen").unwrap();
        let mut buf = [0; 4];
        read(&b, &store, PAGE as u32 + 8, &mut buf).unwrap();
        assert_eq!(&buf, b"seen");
        pages.read_at(&mut buf, PAGE + 8).unwrap();
        assert_eq!(&buf, b"seen");

        unmap_shared(&a, LOW_GUARD, &ra, &store, 3 * PAGE as u32, PAGE).unwrap();
        read(&a, &store, 3 * PAGE as u32 + 8, &mut buf).unwrap();
        assert_eq!(buf, [0; 4]);
        read(&b, &store, PAGE as u32 + 8, &mut buf).unwrap();
        assert_eq!(&buf, b"seen");
    }

    #[cfg(all(
        feature = "sys",
        any(target_os = "linux", target_os = "android", target_os = "macos")
    ))]
    #[test]
    fn shared_pages_never_cover_the_guard_or_reach_past_memory() {
        use wasixv2_pages::Memfd;
        use wasmer::{MemoryType, Store};

        const PAGE: u64 = 65536;
        let mut store = Store::default();
        let memory = Memory::new(&mut store, MemoryType::new(2, None, false)).unwrap();
        let pages = Memfd::new(2 * PAGE).unwrap();
        let read_only = ReadOnly::default();
        let map = |addr: u32, len: u64| {
            map_shared(
                &memory,
                LOW_GUARD,
                &read_only,
                &store,
                (addr, len),
                &pages,
                0,
                true,
            )
        };
        assert_eq!(map(0, PAGE), Err(Errno::EINVAL));
        assert_eq!(map(PAGE as u32, 2 * PAGE), Err(Errno::EINVAL));
        assert_eq!(map(PAGE as u32, PAGE), Ok(()));
    }

    #[cfg(all(
        feature = "sys",
        any(target_os = "linux", target_os = "android", target_os = "macos")
    ))]
    #[test]
    fn a_read_only_view_shows_other_stores_and_takes_no_host_write() {
        use wasixv2_pages::Memfd;
        use wasmer::{MemoryType, Store};

        const PAGE: u64 = 65536;
        let mut store = Store::default();
        let mut memory = || Memory::new(&mut store, MemoryType::new(4, None, true)).unwrap();
        let (writer, reader) = (memory(), memory());
        let (rw, ro) = (ReadOnly::default(), ReadOnly::default());
        let pages = Memfd::new(PAGE).unwrap();
        let at = 2 * PAGE as u32;
        map_shared(&writer, 0, &rw, &store, (at, PAGE), &pages, 0, true).unwrap();
        map_shared(&reader, 0, &ro, &store, (at, PAGE), &pages, 0, false).unwrap();

        write(&writer, 0, &rw, &store, at + 4, b"shown").unwrap();
        let mut buf = [0; 5];
        read(&reader, &store, at + 4, &mut buf).unwrap();
        assert_eq!(&buf, b"shown");
        assert_eq!(write(&reader, 0, &ro, &store, at + 4, b"x"), Err(Fault));
        // A write reaching into the view from below faults as a whole.
        assert_eq!(write(&reader, 0, &ro, &store, at - 2, b"xyz"), Err(Fault));
        write(&reader, 0, &ro, &store, at - 2, b"xy").unwrap();

        // Unmapped, the range is writable private memory again.
        unmap_shared(&reader, 0, &ro, &store, at, PAGE).unwrap();
        write(&reader, 0, &ro, &store, at + 4, b"x").unwrap();
        read(&writer, &store, at + 4, &mut buf).unwrap();
        assert_eq!(&buf, b"shown");
    }

    #[cfg(all(
        feature = "sys",
        any(target_os = "linux", target_os = "android", target_os = "macos")
    ))]
    #[test]
    fn a_guest_store_into_a_read_only_view_traps() {
        use wasixv2_pages::Memfd;
        use wasmer::{Function, MemoryType, Module, Store, TypedFunction, imports};

        const PAGE: u64 = 65536;
        let mut store = Store::default();
        let memory = Memory::new(&mut store, MemoryType::new(4, None, false)).unwrap();
        let wasm = wat::parse_str(
            r#"(module
                (import "env" "memory" (memory 4))
                (func (export "store") (param i32)
                    (i32.store8 (local.get 0) (i32.const 1))))"#,
        )
        .unwrap();
        let module = Module::new(&store, wasm).unwrap();
        let imports = imports! { "env" => { "memory" => memory.clone() } };
        let instance = Instance::new(&mut store, &module, &imports).unwrap();
        let read_only = ReadOnly::default();
        let pages = Memfd::new(PAGE).unwrap();
        let at = 2 * PAGE as u32;
        map_shared(&memory, 0, &read_only, &store, (at, PAGE), &pages, 0, false).unwrap();

        let function: &Function = instance.exports.get_function("store").unwrap();
        let store_at: TypedFunction<u32, ()> = function.typed(&store).unwrap();
        store_at.call(&mut store, at + PAGE as u32).unwrap();
        assert!(store_at.call(&mut store, at + 8).is_err());
        assert_eq!(memory.view(&store).read_u8(u64::from(at) + 8).unwrap(), 0);
    }

    #[cfg(all(unix, feature = "sys"))]
    #[test]
    fn views_are_recorded_and_forgotten_by_range() {
        let mut ranges = Vec::new();
        set_read_only(&mut ranges, 10..20, true);
        set_read_only(&mut ranges, 30..40, true);
        assert!(in_read_only(&ranges, 19, 1));
        assert!(!in_read_only(&ranges, 20, 10));
        assert!(!in_read_only(&ranges, 10, 0));
        // A writable mapping over the middle of one splits it.
        set_read_only(&mut ranges, 12..14, false);
        assert_eq!(ranges, [10..12, 14..20, 30..40]);
        // One across two takes their overlapping parts.
        set_read_only(&mut ranges, 16..35, true);
        assert_eq!(ranges, [10..12, 14..16, 35..40, 16..35]);
        set_read_only(&mut ranges, 0..100, false);
        assert!(ranges.is_empty());
    }

    #[test]
    fn only_writes_that_touch_the_guard_fault() {
        assert!(in_guard(LOW_GUARD, 0, 1));
        assert!(in_guard(LOW_GUARD, LOW_GUARD - 1, 8));
        assert!(!in_guard(LOW_GUARD, LOW_GUARD, 8));
        // An empty access touches nothing.
        assert!(!in_guard(LOW_GUARD, 0, 0));
        // Without a guard nothing faults here.
        assert!(!in_guard(0, 0, 4));
    }
}
