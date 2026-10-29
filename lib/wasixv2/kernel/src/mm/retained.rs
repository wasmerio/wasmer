//! Registration references to pages (`io_uring/memmap.c:io_create_region/io_pin_pages`).
//! Private pages are copied before `do_vmi_munmap` discards their last mapping;
//! `move_vma` moves the live reference instead. Shared mappings retain their object.

use super::{Mm, PAGE_SIZE};
use crate::{Kernel, sync::MutexExt, uidgid::Kuid};
use futures_util::FutureExt;
use futures_util::lock::Mutex as AsyncMutex;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
};
use wasixv2_pages::SharedPages;
use wasixv2_syscalls::{Caller, Errno, Fault, LocalFuture, UserMemory};

#[derive(Default)]
pub(super) struct RetainedPages(Mutex<BTreeMap<u64, Weak<Page>>>);

/// Linux io_uring/rsrc.c:__io_account_mem uses user->locked_vm, separately
/// from the mm->locked_vm that mm/mlock.c accounts for mlock.
#[derive(Default)]
pub(crate) struct MemlockUsers(Mutex<BTreeMap<Kuid, Weak<AtomicU64>>>);

struct MemlockCharge(Arc<AtomicU64>);

impl MemlockUsers {
    fn charge(&self, uid: Kuid, limit: u64) -> Result<MemlockCharge, Errno> {
        let count = {
            let mut users = self.0.lock_or_recover();
            users.retain(|_, count| count.strong_count() != 0);
            match users.get(&uid).and_then(Weak::upgrade) {
                Some(count) => count,
                None => {
                    let count = Arc::new(AtomicU64::new(0));
                    users.insert(uid, Arc::downgrade(&count));
                    count
                }
            }
        };
        count
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                n.checked_add(1).filter(|&n| n <= limit)
            })
            .map_err(|_| Errno::ENOMEM)?;
        Ok(MemlockCharge(count))
    }
}

impl Drop for MemlockCharge {
    fn drop(&mut self) {
        // io_uring/rsrc.c:io_unaccount_mem, on the final pin reference.
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

struct Page {
    kind: PageKind,
    // Acquired before reserving a spare: retained host copies cost at most the
    // accounted number of guest pages, including after munmap or ring inheritance.
    _charge: Option<MemlockCharge>,
}

enum PageKind {
    Local(AsyncMutex<Source>),
    /// P2's foreign handle copies synchronously from any host thread. The same
    /// source lock orders timer CQ writes against munmap/mremap detachment.
    Foreign {
        memory: Mutex<Option<Arc<dyn UserMemory + Send + Sync>>>,
        source: Mutex<Source>,
    },
}
enum Source {
    Live {
        addr: u32,
        spare: Vec<u8>,
    },
    Owned(Vec<u8>),
    Shared {
        pages: Arc<dyn SharedPages>,
        offset: u64,
    },
}

enum LockedSource<'a> {
    Local(futures_util::lock::MutexGuard<'a, Source>),
    Foreign(std::sync::MutexGuard<'a, Source>),
}

impl LockedSource<'_> {
    fn source(&mut self) -> &mut Source {
        match self {
            Self::Local(source) => source,
            Self::Foreign(source) => source,
        }
    }
}

pub(crate) struct Retained {
    pub(crate) addr: u32,
    pub(crate) len: u32,
    owner: u64,
    pages: Vec<Arc<Page>>,
}

impl Kernel {
    /// Like FOLL_WRITE|FOLL_LONGTERM: validate now; later mprotect/munmap do not
    /// invalidate a registration. The mm holds only weak references to these pages.
    pub(crate) async fn retain_pages(
        &self,
        caller: &Caller<'_>,
        addr: u32,
        len: u32,
    ) -> Result<Arc<Retained>, Errno> {
        let mm = self.mm(caller.task());
        let _op = mm.op.lock().await;
        let end = u64::from(addr) + u64::from(len);
        if len == 0
            || addr < caller.memory().low_guard().await
            || end > caller.memory().size().await
        {
            return Err(Errno::EFAULT);
        }
        mm.access
            .check(addr, len as usize, true)
            .map_err(|_| Errno::EFAULT)?;
        let limit = self.lock_limit(caller);
        let uid = self.process(caller.task()).cred().user.real;
        let mut records = Vec::new();
        for base in (u64::from(addr) / PAGE_SIZE..end.div_ceil(PAGE_SIZE)).map(|p| p * PAGE_SIZE) {
            // pin_user_pages(FOLL_WRITE | FOLL_LONGTERM) validates access without
            // modifying user bytes (Linux mm/gup.c:check_vma_flags).
            let existing = mm
                .retained
                .0
                .lock_or_recover()
                .get(&base)
                .and_then(Weak::upgrade);
            let page = if let Some(page) = existing {
                page
            } else {
                let charge = if limit.exempt {
                    None
                } else {
                    Some(self.memlock_users.charge(uid, limit.pages())?)
                };
                let shared = {
                    let space = mm.space();
                    space.mappings.containing(base).and_then(|mapping| {
                        mapping
                            .mapped()
                            .map(|m| (m.pages.clone(), m.offset + base - mapping.start))
                    })
                };
                let source = if let Some((pages, offset)) = shared {
                    Source::Shared { pages, offset }
                } else {
                    let mut spare = Vec::new();
                    spare
                        .try_reserve_exact(PAGE_SIZE as usize)
                        .map_err(|_| Errno::ENOMEM)?;
                    spare.resize(PAGE_SIZE as usize, 0);
                    Source::Live {
                        addr: base as u32,
                        spare,
                    }
                };
                let kind = match mm.foreign() {
                    Some(memory) => PageKind::Foreign {
                        memory: Mutex::new(Some(memory)),
                        source: Mutex::new(source),
                    },
                    None => PageKind::Local(AsyncMutex::new(source)),
                };
                let page = Arc::new(Page {
                    kind,
                    _charge: charge,
                });
                mm.retained
                    .0
                    .lock_or_recover()
                    .insert(base, Arc::downgrade(&page));
                page
            };
            records.push(page);
        }
        Ok(Arc::new(Retained {
            addr,
            len,
            owner: mm.id(),
            pages: records,
        }))
    }
}

impl Mm {
    /// Called under Mm::op before any old bytes or mappings are discarded.
    pub(super) async fn retain_unmapped(&self, memory: &dyn UserMemory, start: u64, end: u64) {
        let pages: Vec<_> = {
            let mut table = self.retained.0.lock_or_recover();
            let keys: Vec<_> = table.range(start..end).map(|(&at, _)| at).collect();
            keys.into_iter()
                .filter_map(|at| table.remove(&at).and_then(|p| p.upgrade()))
                .collect()
        };
        for page in pages {
            match &page.kind {
                PageKind::Local(source) => {
                    let mut source = source.lock().await;
                    if let Source::Live { addr, spare } = &mut *source {
                        if memory.force_read(*addr, spare).await.is_err() {
                            tracing::error!(
                                address = *addr,
                                "failed to copy retained page during detachment"
                            );
                            spare.fill(0);
                        }
                        *source = Source::Owned(std::mem::take(spare));
                    }
                }
                PageKind::Foreign { memory, source } => {
                    let mut source = source.lock_or_recover();
                    if let Source::Live { addr, spare } = &mut *source {
                        let memory = memory.lock_or_recover();
                        let memory = memory.as_ref().ok_or(Fault);
                        if memory
                            .and_then(|memory| sync_copy(memory.force_read(*addr, spare)))
                            .is_err()
                        {
                            tracing::error!(
                                address = *addr,
                                "failed to copy foreign retained page during detachment"
                            );
                            spare.fill(0);
                        }
                        *source = Source::Owned(std::mem::take(spare));
                    }
                    // Linux pins only the page, not the entire dead mm. Owned
                    // bytes and SharedPages no longer need the image's handle.
                    memory.lock_or_recover().take();
                }
            }
        }
    }

    /// mm/mremap.c:move_vma moves the pinned physical pages. Serialize the
    /// equivalent copy and mapping relocation with every retained transfer.
    pub(super) async fn retain_moved(
        &self,
        start: u64,
        len: u64,
        target: u64,
        moving: impl std::future::Future<Output = Result<(), Errno>>,
    ) -> Result<(), Errno> {
        let pages: Vec<_> = self
            .retained
            .0
            .lock_or_recover()
            .range(start..start + len)
            .filter_map(|(&at, page)| page.upgrade().map(|page| (at, page)))
            .collect();
        let mut held = Vec::with_capacity(pages.len());
        // Always lock in address order, including when a registration spans pages.
        for (_, page) in &pages {
            held.push(match &page.kind {
                PageKind::Local(source) => LockedSource::Local(source.lock().await),
                PageKind::Foreign { source, .. } => LockedSource::Foreign(source.lock_or_recover()),
            });
        }
        moving.await?;
        let mut table = self.retained.0.lock_or_recover();
        for ((at, page), source) in pages.iter().zip(&mut held) {
            let new = target + at - start;
            if let Source::Live { addr, .. } = source.source() {
                *addr = new as u32;
            }
            table.remove(at);
            table.insert(new, Arc::downgrade(page));
        }
        Ok(())
    }
}

impl Retained {
    /// Linux pins struct pages, not the issuer's virtual addresses. Foreign
    /// handles keep addressing the registrant; a local borrowed memory cannot.
    pub(crate) fn check_mm(&self, mm: u64) -> Result<(), Errno> {
        if self.owner == mm || self.synchronous() {
            Ok(())
        } else {
            Err(Errno::EFAULT)
        }
    }

    pub(crate) fn synchronous(&self) -> bool {
        self.pages
            .iter()
            .all(|page| matches!(&page.kind, PageKind::Foreign { .. }))
    }

    /// Pinned ring access from io_uring's timer/wake handlers. No guest store is borrowed.
    pub(crate) fn read_sync(&self, offset: u32, out: &mut [u8]) -> Result<(), Fault> {
        self.transfer_sync(offset, out, false)
    }

    pub(crate) fn write_sync(&self, offset: u32, data: &[u8]) -> Result<(), Fault> {
        self.transfer_sync(offset, &mut data.to_vec(), true)
    }

    fn transfer_sync(&self, offset: u32, buf: &mut [u8], write: bool) -> Result<(), Fault> {
        if u64::from(offset) + buf.len() as u64 > u64::from(self.len) {
            return Err(Fault);
        }
        let mut done = 0;
        while done < buf.len() {
            let position =
                u64::from(self.addr % PAGE_SIZE as u32) + u64::from(offset) + done as u64;
            let index = (position / PAGE_SIZE) as usize;
            let skip = (position % PAGE_SIZE) as usize;
            let n = (PAGE_SIZE as usize - skip).min(buf.len() - done);
            let PageKind::Foreign { memory, source } = &self.pages[index].kind else {
                return Err(Fault);
            };
            let mut source = source.lock_or_recover();
            let memory = memory.lock_or_recover();
            transfer_sync(
                &mut source,
                memory.as_deref().map(|m| m as &dyn UserMemory),
                skip,
                &mut buf[done..done + n],
                write,
            )?;
            done += n;
        }
        Ok(())
    }

    pub(crate) async fn read(
        &self,
        memory: &dyn UserMemory,
        offset: u32,
        out: &mut [u8],
    ) -> Result<(), Fault> {
        self.transfer(memory, offset, out, false).await
    }

    pub(crate) async fn write(
        &self,
        memory: &dyn UserMemory,
        offset: u32,
        data: &[u8],
    ) -> Result<(), Fault> {
        self.transfer(memory, offset, &mut data.to_vec(), true)
            .await
    }

    async fn transfer(
        &self,
        memory: &dyn UserMemory,
        offset: u32,
        buf: &mut [u8],
        write: bool,
    ) -> Result<(), Fault> {
        if u64::from(offset) + buf.len() as u64 > u64::from(self.len) {
            return Err(Fault);
        }
        let mut done = 0;
        while done < buf.len() {
            let position =
                u64::from(self.addr % PAGE_SIZE as u32) + u64::from(offset) + done as u64;
            let index = (position / PAGE_SIZE) as usize;
            let skip = (position % PAGE_SIZE) as usize;
            let n = (PAGE_SIZE as usize - skip).min(buf.len() - done);
            let bytes = &mut buf[done..done + n];
            if let PageKind::Foreign { memory, source } = &self.pages[index].kind {
                let mut source = source.lock_or_recover();
                let memory = memory.lock_or_recover();
                transfer_sync(
                    &mut source,
                    memory.as_deref().map(|m| m as &dyn UserMemory),
                    skip,
                    bytes,
                    write,
                )?;
                done += n;
                continue;
            }
            let PageKind::Local(source) = &self.pages[index].kind else {
                unreachable!()
            };
            let mut source = source.lock().await;
            match &mut *source {
                Source::Live { addr, .. } => {
                    if write {
                        memory.force_write(*addr + skip as u32, bytes).await?;
                    } else {
                        memory.force_read(*addr + skip as u32, bytes).await?;
                    }
                }
                Source::Owned(data) => {
                    if write {
                        data[skip..skip + n].copy_from_slice(bytes);
                    } else {
                        bytes.copy_from_slice(&data[skip..skip + n]);
                    }
                }
                Source::Shared { pages, offset } => {
                    if write {
                        pages
                            .write_at(bytes, *offset + skip as u64)
                            .map_err(|_| Fault)?;
                    } else {
                        pages
                            .read_at(bytes, *offset + skip as u64)
                            .map_err(|_| Fault)?;
                    }
                }
            }
            done += n;
        }
        Ok(())
    }
}

/// ForeignMemory's copies are immediate (exec/memory.rs); never block a guest executor.
fn sync_copy(future: LocalFuture<'_, Result<(), Fault>>) -> Result<(), Fault> {
    // A future handle that stops completing immediately must fault, not panic
    // a timer thread or an mm teardown path.
    future.now_or_never().unwrap_or(Err(Fault))
}

fn transfer_sync(
    source: &mut Source,
    memory: Option<&dyn UserMemory>,
    skip: usize,
    bytes: &mut [u8],
    write: bool,
) -> Result<(), Fault> {
    match source {
        Source::Live { addr, .. } => {
            let memory = memory.ok_or(Fault)?;
            if write {
                sync_copy(memory.force_write(*addr + skip as u32, bytes))
            } else {
                sync_copy(memory.force_read(*addr + skip as u32, bytes))
            }
        }
        Source::Owned(data) => {
            if write {
                data[skip..skip + bytes.len()].copy_from_slice(bytes);
            } else {
                bytes.copy_from_slice(&data[skip..skip + bytes.len()]);
            }
            Ok(())
        }
        Source::Shared { pages, offset } => if write {
            pages.write_at(bytes, *offset + skip as u64)
        } else {
            pages.read_at(bytes, *offset + skip as u64)
        }
        .map_err(|_| Fault),
    }
}

/// Kernel-local redirection for the existing vectored I/O paths.
pub(crate) struct RetainedMemory<'a> {
    pub(crate) memory: &'a dyn UserMemory,
    pub(crate) range: &'a Retained,
}

impl UserMemory for RetainedMemory<'_> {
    fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        Box::pin(async move {
            if addr >= self.range.addr
                && u64::from(addr) + buf.len() as u64
                    <= u64::from(self.range.addr) + u64::from(self.range.len)
            {
                self.range
                    .read(self.memory, addr - self.range.addr, buf)
                    .await
            } else {
                self.memory.read(addr, buf).await
            }
        })
    }
    fn write<'a>(&'a self, addr: u32, data: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        Box::pin(async move {
            if addr >= self.range.addr
                && u64::from(addr) + data.len() as u64
                    <= u64::from(self.range.addr) + u64::from(self.range.len)
            {
                self.range
                    .write(self.memory, addr - self.range.addr, data)
                    .await
            } else {
                self.memory.write(addr, data).await
            }
        })
    }
    fn size(&self) -> LocalFuture<'_, u64> {
        self.memory.size()
    }
    fn grow(&self, pages: u32) -> LocalFuture<'_, Option<u64>> {
        self.memory.grow(pages)
    }
    fn force_read<'a>(
        &'a self,
        addr: u32,
        buf: &'a mut [u8],
    ) -> LocalFuture<'a, Result<(), Fault>> {
        self.memory.force_read(addr, buf)
    }
    fn force_write<'a>(&'a self, addr: u32, data: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        self.memory.force_write(addr, data)
    }
}
