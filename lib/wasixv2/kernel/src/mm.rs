//! The program break and anonymous mappings inside the guest's linear memory.
//!
//! Linear memory only grows, so `brk` and `mmap` hand out pages above everything the image owns
//! (its data, stack and heap) and grow memory when no unmapped run fits. `munmap` returns pages
//! for reuse by later mappings, which are zeroed then. Wasm has no page protection: unmapped
//! pages stay accessible and `mprotect` changes nothing, except that executable memory is
//! refused, since wasm cannot run code generated at run time.

use wasixv2_abi::{
    Buf, Fd, Loff, MadviseAdvice, MapFlags, MclFlags, MlockFlags, MremapFlags, MsyncFlags,
    ProtFlags, Ulong, Uptr, Usize,
};
use wasixv2_syscalls::{Caller, Errno, SysResult, UserMemory, WASM_PAGE_SIZE};

use crate::Kernel;

/// The page size the guest sees in `AT_PAGESZ` and the unit of every mapping.
pub(crate) const PAGE_SIZE: u64 = WASM_PAGE_SIZE;

#[derive(Default)]
pub(crate) struct AddressSpace {
    /// First page under the allocator's control, fixed by the first growth. Pages below belong to
    /// the image.
    base: Option<u64>,
    /// For each page from `base` to the end of what the allocator knows: mapped or not.
    mapped: Vec<bool>,
    /// The program break: where it started and where it is.
    brk: Option<(u64, u64)>,
}

/// Where a new mapping goes.
pub(crate) enum Placement {
    /// Reuses unmapped pages at this address, which must be zeroed.
    Reuse(u64),
    /// Needs this many new pages.
    Grow(u32),
}

impl AddressSpace {
    /// Picks the first unmapped run of `pages` pages, marking it mapped.
    pub(crate) fn place(&mut self, pages: u64) -> Result<Placement, Errno> {
        if let Some(index) = self.free_run(pages) {
            self.mapped[index..][..pages as usize].fill(true);
            let base = self.base.expect("pages are tracked only after a growth");
            return Ok(Placement::Reuse((base + index as u64) * PAGE_SIZE));
        }
        let pages = u32::try_from(pages).map_err(|_| Errno::ENOMEM)?;
        Ok(Placement::Grow(pages))
    }

    /// Records that memory grew from `previous` bytes by `pages` pages for a new mapping.
    pub(crate) fn grown(&mut self, previous: u64, pages: u32) {
        let first = previous / PAGE_SIZE;
        let base = *self.base.get_or_insert(first);
        let index = (first - base) as usize;
        debug_assert!(self.mapped.len() <= index, "memory never shrinks");
        // Pages someone else grew in between count as mapped, so they are never handed out.
        self.mapped.resize(index + pages as usize, true);
    }

    /// Unmaps the pages of `[addr, addr + len)` that the allocator handed out.
    pub(crate) fn unmap(&mut self, addr: u64, len: u64) {
        self.set_pages(addr / PAGE_SIZE, (addr + len).div_ceil(PAGE_SIZE), false);
    }

    /// Marks the allocator's pages in `[first, end)` as mapped or not.
    fn set_pages(&mut self, first: u64, end: u64, mapped: bool) {
        let Some(base) = self.base else { return };
        let end = end.min(base + self.mapped.len() as u64);
        for page in first.max(base)..end {
            self.mapped[(page - base) as usize] = mapped;
        }
    }

    /// Whether `page` is in use: the image's pages, pages handed out, and pages grown by
    /// someone else; nothing at or past `mem_pages`.
    fn page_mapped(&self, page: u64, mem_pages: u64) -> bool {
        if page >= mem_pages {
            return false;
        }
        match self.base {
            Some(base) if page >= base => self
                .mapped
                .get((page - base) as usize)
                .copied()
                .unwrap_or(true),
            _ => true,
        }
    }

    /// Whether every page of `[addr, addr + len)` is in use.
    pub(crate) fn range_mapped(&self, addr: u64, len: u64, mem_size: u64) -> bool {
        let mem_pages = mem_size / PAGE_SIZE;
        (addr / PAGE_SIZE..(addr + len).div_ceil(PAGE_SIZE))
            .all(|page| self.page_mapped(page, mem_pages))
    }

    /// Whether `[first, end)` can be taken: each page is the allocator's and free, or at or past
    /// the end of memory.
    fn pages_free(&self, first: u64, end: u64, mem_pages: u64) -> bool {
        let movable = |page: u64| self.base.is_some_and(|base| page >= base) || page >= mem_pages;
        (first..end).all(|page| movable(page) && !self.page_mapped(page, mem_pages))
    }

    fn free_run(&self, pages: u64) -> Option<usize> {
        let pages = usize::try_from(pages).ok()?;
        let mut run = 0;
        for (index, &mapped) in self.mapped.iter().enumerate() {
            run = if mapped { 0 } else { run + 1 };
            if run == pages {
                return Some(index + 1 - pages);
            }
        }
        None
    }
}

/// Zeroes `[addr, addr + len)`, as a fresh anonymous mapping must read.
pub(crate) async fn zero(memory: &dyn UserMemory, addr: u64, len: u64) -> Result<(), Errno> {
    let zeroes = vec![0; PAGE_SIZE as usize];
    let mut pos = addr;
    while pos < addr + len {
        let n = (addr + len - pos).min(PAGE_SIZE);
        let at = u32::try_from(pos).map_err(|_| Errno::ENOMEM)?;
        memory
            .write(at, &zeroes[..n as usize])
            .await
            .map_err(|_| Errno::ENOMEM)?;
        pos += n;
    }
    Ok(())
}

/// Copies `len` bytes from `from` to `to`, which do not overlap.
async fn copy(memory: &dyn UserMemory, from: u64, to: u64, len: u64) -> Result<(), Errno> {
    let mut buf = vec![0; PAGE_SIZE as usize];
    let mut done = 0;
    while done < len {
        let n = (len - done).min(PAGE_SIZE) as usize;
        let fault = |_| Errno::EFAULT;
        memory
            .read((from + done) as u32, &mut buf[..n])
            .await
            .map_err(fault)?;
        memory
            .write((to + done) as u32, &buf[..n])
            .await
            .map_err(fault)?;
        done += n as u64;
    }
    Ok(())
}

/// Pages a mapping of `len` bytes needs; `EINVAL` for zero, as Linux.
pub(crate) fn pages(len: u32) -> Result<u64, Errno> {
    match u64::from(len).div_ceil(PAGE_SIZE) {
        0 => Err(Errno::EINVAL),
        pages => Ok(pages),
    }
}

fn check_aligned(addr: u32) -> Result<(), Errno> {
    match u64::from(addr).is_multiple_of(PAGE_SIZE) {
        true => Ok(()),
        false => Err(Errno::EINVAL),
    }
}

/// `MADV_*` values Linux's `madvise_behavior_valid` accepts.
fn advice_valid(advice: MadviseAdvice) -> bool {
    matches!(advice.0, 0..=4 | 8..=25 | 100..=103)
}

impl Kernel {
    async fn mem_size(&self, caller: &Caller<'_>) -> u64 {
        caller.memory().size().await
    }

    /// Fails with `ENOMEM` unless all of `[addr, addr + len)` is mapped.
    async fn check_mapped(&self, caller: &Caller<'_>, addr: u64, len: u64) -> Result<(), Errno> {
        let size = self.mem_size(caller).await;
        let process = self.process(caller.task());
        match process.mm.lock().unwrap().range_mapped(addr, len, size) {
            true => Ok(()),
            false => Err(Errno::ENOMEM),
        }
    }

    /// Takes the pages `[first, end)` for a mapping or the break, growing memory for those past
    /// its end, and zeroes those that were mapped before. They must be free.
    async fn take_pages(&self, caller: &Caller<'_>, first: u64, end: u64) -> Result<(), Errno> {
        let mem_pages = self.mem_size(caller).await / PAGE_SIZE;
        let process = self.process(caller.task());
        if end > mem_pages {
            let grow = u32::try_from(end - mem_pages).map_err(|_| Errno::ENOMEM)?;
            let previous = caller.memory().grow(grow).await.ok_or(Errno::ENOMEM)?;
            let mut mm = process.mm.lock().unwrap();
            mm.grown(previous, grow);
            if previous != mem_pages * PAGE_SIZE {
                // Someone else grew memory in between; give up rather than guess.
                mm.set_pages(
                    previous / PAGE_SIZE,
                    previous / PAGE_SIZE + grow as u64,
                    false,
                );
                return Err(Errno::ENOMEM);
            }
        }
        process.mm.lock().unwrap().set_pages(first, end, true);
        let reused_end = end.min(mem_pages);
        if first < reused_end {
            zero(
                caller.memory(),
                first * PAGE_SIZE,
                (reused_end - first) * PAGE_SIZE,
            )
            .await?;
        }
        Ok(())
    }

    /// The break starts at the end of memory as it is on the first call.
    pub(crate) async fn sys_brk(&self, caller: &Caller<'_>, brk: Uptr) -> SysResult<i32> {
        let size = self.mem_size(caller).await;
        let process = self.process(caller.task());
        let (start, current) = *process.mm.lock().unwrap().brk.get_or_insert((size, size));
        let new = u64::from(brk);
        if new < start {
            return Ok(current as i32);
        }
        let (old_end, new_end) = (current.div_ceil(PAGE_SIZE), new.div_ceil(PAGE_SIZE));
        if new_end > old_end {
            let free = process
                .mm
                .lock()
                .unwrap()
                .pages_free(old_end, new_end, size / PAGE_SIZE);
            if !free || self.take_pages(caller, old_end, new_end).await.is_err() {
                return Ok(current as i32);
            }
        } else {
            process
                .mm
                .lock()
                .unwrap()
                .set_pages(new_end, old_end, false);
        }
        process.mm.lock().unwrap().brk = Some((start, new));
        Ok(new as i32)
    }

    /// Anonymous mappings only; see the module docs.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn sys_mmap(
        &self,
        caller: &Caller<'_>,
        addr: Uptr,
        len: Ulong,
        prot: ProtFlags,
        flags: MapFlags,
        fd: Fd,
        offset: Loff,
    ) -> SysResult<Uptr> {
        if offset % PAGE_SIZE as i64 != 0 {
            return Err(Errno::EINVAL.into());
        }
        if !flags.contains(MapFlags::MAP_ANONYMOUS) {
            self.file(caller.task(), fd)?;
        }
        if prot.contains(ProtFlags::PROT_EXEC) {
            return Err(Errno::EACCES.into());
        }
        // Shared anonymous memory is ordinary memory within one address space. Wasm modules
        // cannot share part of their memory, so no other process will ever see it.
        match flags & MapFlags::MAP_TYPE {
            MapFlags::MAP_SHARED | MapFlags::MAP_PRIVATE | MapFlags::MAP_SHARED_VALIDATE => {}
            _ => return Err(Errno::EINVAL.into()),
        }
        if !flags.contains(MapFlags::MAP_ANONYMOUS) {
            // TODO: private file mappings.
            return Err(Errno::ENODEV.into());
        }
        let pages = pages(len)?;
        if flags.intersects(MapFlags::MAP_FIXED | MapFlags::MAP_FIXED_NOREPLACE) {
            let noreplace = !flags.contains(MapFlags::MAP_FIXED);
            return self.map_fixed(caller, addr, pages, noreplace).await;
        }
        let process = self.process(caller.task());
        let placement = process.mm.lock().unwrap().place(pages)?;
        let addr = match placement {
            Placement::Reuse(addr) => {
                zero(caller.memory(), addr, pages * PAGE_SIZE).await?;
                addr
            }
            Placement::Grow(pages) => {
                let previous = caller.memory().grow(pages).await.ok_or(Errno::ENOMEM)?;
                process.mm.lock().unwrap().grown(previous, pages);
                previous
            }
        };
        Ok(addr as Uptr)
    }

    /// `MAP_FIXED` works over pages the allocator owns and past the end of memory; the image's
    /// own pages cannot be replaced.
    async fn map_fixed(
        &self,
        caller: &Caller<'_>,
        addr: Uptr,
        pages: u64,
        noreplace: bool,
    ) -> SysResult<Uptr> {
        check_aligned(addr)?;
        let first = u64::from(addr) / PAGE_SIZE;
        let end = first + pages;
        if end * PAGE_SIZE > 1 << 32 {
            return Err(Errno::ENOMEM.into());
        }
        let mem_pages = self.mem_size(caller).await / PAGE_SIZE;
        let process = self.process(caller.task());
        {
            let mut mm = process.mm.lock().unwrap();
            if mm.pages_free(first, end, mem_pages) {
                // Free pages can be taken as they are.
            } else if noreplace {
                return Err(Errno::EEXIST.into());
            } else {
                let owned = |page: u64| mm.base.is_some_and(|base| page >= base);
                if !(first..end).all(|page| owned(page) || page >= mem_pages) {
                    return Err(Errno::EINVAL.into());
                }
                mm.set_pages(first, end, false);
            }
        }
        // Pages between the end of memory and the mapping stay free.
        self.take_pages(caller, first, end).await?;
        if first > mem_pages {
            process
                .mm
                .lock()
                .unwrap()
                .set_pages(mem_pages, first, false);
        }
        Ok(addr)
    }

    pub(crate) async fn sys_munmap(
        &self,
        caller: &Caller<'_>,
        addr: Uptr,
        len: Usize,
    ) -> SysResult<i32> {
        check_aligned(addr)?;
        if len == 0 {
            return Err(Errno::EINVAL.into());
        }
        let process = self.process(caller.task());
        process.mm.lock().unwrap().unmap(addr.into(), len.into());
        Ok(0)
    }

    pub(crate) async fn sys_mremap(
        &self,
        caller: &Caller<'_>,
        addr: Uptr,
        old_len: Ulong,
        new_len: Ulong,
        flags: MremapFlags,
        new_addr: Uptr,
    ) -> SysResult<i32> {
        let known = MremapFlags::MREMAP_MAYMOVE | MremapFlags::MREMAP_FIXED;
        let may_move = flags.contains(MremapFlags::MREMAP_MAYMOVE);
        if !(flags & !known).is_empty() || (flags.contains(MremapFlags::MREMAP_FIXED) && !may_move)
        {
            return Err(Errno::EINVAL.into());
        }
        check_aligned(addr)?;
        if new_len == 0 || old_len == 0 {
            return Err(Errno::EINVAL.into());
        }
        let (addr64, old_pages, new_pages) = (u64::from(addr), pages(old_len)?, pages(new_len)?);
        self.check_mapped(caller, addr64, old_pages * PAGE_SIZE)
            .await
            .map_err(|_| Errno::EFAULT)?;
        let process = self.process(caller.task());
        let first = addr64 / PAGE_SIZE;
        if flags.contains(MremapFlags::MREMAP_FIXED) {
            let target = self.map_fixed(caller, new_addr, new_pages, false).await?;
            let keep = old_pages.min(new_pages) * PAGE_SIZE;
            copy(caller.memory(), addr64, target.into(), keep).await?;
            process
                .mm
                .lock()
                .unwrap()
                .unmap(addr64, old_pages * PAGE_SIZE);
            return Ok(target as i32);
        }
        if new_pages <= old_pages {
            let mut mm = process.mm.lock().unwrap();
            mm.set_pages(first + new_pages, first + old_pages, false);
            return Ok(addr as i32);
        }
        let mem_pages = self.mem_size(caller).await / PAGE_SIZE;
        let grow_in_place =
            process
                .mm
                .lock()
                .unwrap()
                .pages_free(first + old_pages, first + new_pages, mem_pages);
        if grow_in_place
            && self
                .take_pages(caller, first + old_pages, first + new_pages)
                .await
                .is_ok()
        {
            return Ok(addr as i32);
        }
        if !may_move {
            return Err(Errno::ENOMEM.into());
        }
        let flags = MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS;
        let len = (new_pages * PAGE_SIZE) as u32;
        let rw = ProtFlags::PROT_READ | ProtFlags::PROT_WRITE;
        let target = self.sys_mmap(caller, 0, len, rw, flags, Fd(-1), 0).await?;
        copy(
            caller.memory(),
            addr64,
            target.into(),
            old_pages * PAGE_SIZE,
        )
        .await?;
        process
            .mm
            .lock()
            .unwrap()
            .unmap(addr64, old_pages * PAGE_SIZE);
        Ok(target as i32)
    }

    pub(crate) async fn sys_mprotect(
        &self,
        caller: &Caller<'_>,
        start: Uptr,
        len: Usize,
        prot: ProtFlags,
    ) -> SysResult<i32> {
        check_aligned(start)?;
        let known = ProtFlags::PROT_READ
            | ProtFlags::PROT_WRITE
            | ProtFlags::PROT_EXEC
            | ProtFlags::PROT_SEM
            | ProtFlags::PROT_GROWSDOWN
            | ProtFlags::PROT_GROWSUP;
        let grows = ProtFlags::PROT_GROWSDOWN | ProtFlags::PROT_GROWSUP;
        if !(prot & !known).is_empty() || prot.contains(grows) {
            return Err(Errno::EINVAL.into());
        }
        if len == 0 {
            return Ok(0);
        }
        self.check_mapped(caller, start.into(), len.into()).await?;
        if prot.contains(ProtFlags::PROT_EXEC) {
            return Err(Errno::EACCES.into());
        }
        Ok(0)
    }

    /// `MADV_DONTNEED` zeroes the pages, as it does to private anonymous memory; every other
    /// advice changes nothing.
    pub(crate) async fn sys_madvise(
        &self,
        caller: &Caller<'_>,
        start: Uptr,
        len: Usize,
        advice: MadviseAdvice,
    ) -> SysResult<i32> {
        check_aligned(start)?;
        if !advice_valid(advice) {
            return Err(Errno::EINVAL.into());
        }
        if len == 0 {
            return Ok(0);
        }
        let len = u64::from(len).div_ceil(PAGE_SIZE) * PAGE_SIZE;
        self.check_mapped(caller, start.into(), len).await?;
        match advice {
            MadviseAdvice::MADV_DONTNEED => zero(caller.memory(), start.into(), len).await?,
            MadviseAdvice::MADV_REMOVE => return Err(Errno::EINVAL.into()),
            MadviseAdvice(100 | 101) => return Err(Errno::EPERM.into()),
            _ => {}
        }
        Ok(0)
    }

    /// Every mapped page is resident.
    pub(crate) async fn sys_mincore(
        &self,
        caller: &Caller<'_>,
        start: Uptr,
        len: Usize,
        vec: Buf,
    ) -> SysResult<i32> {
        check_aligned(start)?;
        self.check_mapped(caller, start.into(), len.into()).await?;
        let pages = u64::from(len).div_ceil(PAGE_SIZE) as usize;
        caller.memory().write(vec.addr(), &vec![1; pages]).await?;
        Ok(0)
    }

    pub(crate) async fn sys_msync(
        &self,
        caller: &Caller<'_>,
        start: Uptr,
        len: Usize,
        flags: MsyncFlags,
    ) -> SysResult<i32> {
        check_aligned(start)?;
        let known = MsyncFlags::MS_ASYNC | MsyncFlags::MS_INVALIDATE | MsyncFlags::MS_SYNC;
        if !(flags & !known).is_empty()
            || flags.contains(MsyncFlags::MS_ASYNC | MsyncFlags::MS_SYNC)
        {
            return Err(Errno::EINVAL.into());
        }
        self.check_mapped(caller, start.into(), len.into()).await?;
        Ok(0)
    }

    /// Memory is never paged out, so locking only checks the range.
    pub(crate) async fn sys_mlock(
        &self,
        caller: &Caller<'_>,
        start: Uptr,
        len: Usize,
        flags: MlockFlags,
    ) -> SysResult<i32> {
        if !(flags & !MlockFlags::MLOCK_ONFAULT).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        let first = u64::from(start) / PAGE_SIZE * PAGE_SIZE;
        let len = u64::from(start) + u64::from(len) - first;
        self.check_mapped(caller, first, len).await?;
        Ok(0)
    }

    pub(crate) fn sys_mlockall(&self, flags: MclFlags) -> SysResult<i32> {
        let known = MclFlags::MCL_CURRENT | MclFlags::MCL_FUTURE | MclFlags::MCL_ONFAULT;
        let lock = MclFlags::MCL_CURRENT | MclFlags::MCL_FUTURE;
        if !(flags & !known).is_empty() || !flags.intersects(lock) {
            return Err(Errno::EINVAL.into());
        }
        Ok(0)
    }
}
