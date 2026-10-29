//! Registration references to pages (`io_uring/memmap.c:io_create_region/io_pin_pages`).
//! Private pages are copied before `do_vmi_munmap` discards their last mapping;
//! `move_vma` moves the live reference instead. Shared mappings retain their object.

use super::{Mm, PAGE_SIZE};
use crate::{Kernel, sync::MutexExt};
use futures_util::FutureExt;
use futures_util::lock::Mutex as AsyncMutex;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
};
use wasixv2_pages::SharedPages;
use wasixv2_syscalls::{Caller, Errno, Fault, LocalFuture, UserMemory};

#[derive(Default)]
pub(super) struct RetainedPages(Mutex<BTreeMap<u64, Weak<Page>>>);

enum Page {
    Local(AsyncMutex<Source>),
    /// P2's foreign handle copies synchronously from any host thread. The same
    /// source lock orders timer CQ writes against munmap/mremap detachment.
    Foreign {
        memory: Arc<dyn UserMemory + Send + Sync>,
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

pub(crate) struct Retained {
    pub(crate) addr: u32,
    pub(crate) len: u32,
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
        let mut records = Vec::new();
        for base in (u64::from(addr) / PAGE_SIZE..end.div_ceil(PAGE_SIZE)).map(|p| p * PAGE_SIZE) {
            let at = base.max(u64::from(addr)) as u32;
            let mut byte = [0];
            caller
                .memory()
                .read(at, &mut byte)
                .await
                .map_err(|_| Errno::EFAULT)?;
            caller
                .memory()
                .write(at, &byte)
                .await
                .map_err(|_| Errno::EFAULT)?;
            let existing = mm
                .retained
                .0
                .lock_or_recover()
                .get(&base)
                .and_then(Weak::upgrade);
            let page = if let Some(page) = existing {
                page
            } else {
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
                let page = Arc::new(match mm.foreign() {
                    Some(memory) => Page::Foreign {
                        memory,
                        source: Mutex::new(source),
                    },
                    None => Page::Local(AsyncMutex::new(source)),
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
            match &*page {
                Page::Local(source) => {
                    let mut source = source.lock().await;
                    if let Source::Live { addr, spare } = &mut *source {
                        memory
                            .force_read(*addr, spare)
                            .await
                            .expect("retained page was validated at registration");
                        *source = Source::Owned(std::mem::take(spare));
                    }
                }
                Page::Foreign { memory, source } => {
                    let mut source = source.lock_or_recover();
                    if let Source::Live { addr, spare } = &mut *source {
                        sync_copy(memory.force_read(*addr, spare))
                            .expect("retained page was validated at registration");
                        *source = Source::Owned(std::mem::take(spare));
                    }
                }
            }
        }
    }

    pub(super) async fn retain_moved(&self, start: u64, len: u64, target: u64) {
        let pages: Vec<_> = {
            let mut table = self.retained.0.lock_or_recover();
            let keys: Vec<_> = table.range(start..start + len).map(|(&at, _)| at).collect();
            keys.into_iter()
                .filter_map(|at| table.remove(&at).and_then(|p| p.upgrade()).map(|p| (at, p)))
                .collect()
        };
        for (at, page) in pages {
            let new = target + at - start;
            match &*page {
                Page::Local(source) => {
                    if let Source::Live { addr, .. } = &mut *source.lock().await {
                        *addr = new as u32;
                    }
                }
                Page::Foreign { source, .. } => {
                    if let Source::Live { addr, .. } = &mut *source.lock_or_recover() {
                        *addr = new as u32;
                    }
                }
            }
            self.retained
                .0
                .lock_or_recover()
                .insert(new, Arc::downgrade(&page));
        }
    }
}

impl Retained {
    pub(crate) fn synchronous(&self) -> bool {
        self.pages
            .iter()
            .all(|page| matches!(&**page, Page::Foreign { .. }))
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
            let Page::Foreign { memory, source } = &*self.pages[index] else {
                return Err(Fault);
            };
            transfer_sync(
                &mut source.lock_or_recover(),
                &**memory,
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
            if let Page::Foreign { memory, source } = &*self.pages[index] {
                transfer_sync(&mut source.lock_or_recover(), &**memory, skip, bytes, write)?;
                done += n;
                continue;
            }
            let Page::Local(source) = &*self.pages[index] else {
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
    future
        .now_or_never()
        .expect("attached foreign memory copies synchronously")
}

fn transfer_sync(
    source: &mut Source,
    memory: &dyn UserMemory,
    skip: usize,
    bytes: &mut [u8],
    write: bool,
) -> Result<(), Fault> {
    match source {
        Source::Live { addr, .. } => {
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
