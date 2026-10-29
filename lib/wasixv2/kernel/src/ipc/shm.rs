//! System V shared memory (`ipc/shm.c`): `shmget`, `shmat`, `shmdt` and `shmctl`.
//!
//! Where the host has shared memory objects ([`Host::shared_pages`]), a segment is one, as
//! Linux's is a shmem file (`shmem_kernel_file_setup` in `newseg`), and every attachment maps
//! its pages into the attaching process's memory ([`UserMemory::map_shared`]): all attachments,
//! in any process, are the same memory, and a futex in the segment works across them. A
//! `SHM_RDONLY` attachment shows them read-only: a store by the guest faults with `SIGSEGV`.
//!
//! [`Host::shared_pages`]: wasixv2_host::Host::shared_pages
//! [`UserMemory::map_shared`]: wasixv2_syscalls::UserMemory::map_shared
//!
//! Elsewhere, or for memory that cannot show a host object's pages, a segment is a kernel
//! buffer of whole pages, allocated as they are first written. Wasm cannot make part of one
//! module's memory appear in another, or one page appear at two addresses, so an attachment is
//! then a copy in the attaching process's memory (see `mm/shared.rs`), synchronised with the
//! segment, byte for byte in both directions, at these points:
//!
//! - `shmat` fills the new attachment from the segment;
//! - `shmdt`, `munmap`, `exit`, `exit_group` and `execve` write the attachment back;
//! - `msync` over an attachment, and `MADV_DONTNEED`, synchronise it;
//! - `semop`/`semtimedop` synchronise every attachment of the caller before and after the
//!   operation; `msgsnd` and `mq_timedsend` before sending, `msgrcv` and `mq_timedreceive`
//!   after receiving.
//!
//! So data handed over under a SysV semaphore or with a message, the way shared memory is
//! normally used between processes, arrives as on Linux. Two copied attachments in one process,
//! or processes that hand data over by other means (a futex in the segment, polling a flag), do
//! not see each other's changes until one of them passes such a point, and futexes in a copied
//! attachment only work between the threads of one process.

use std::{
    collections::HashMap,
    io,
    sync::{Arc, Mutex, MutexGuard},
};

use wasixv2_abi::{GuestPtr, GuestPtrMut, IpcCmd, RlimitResource, ShmFlags, Shmid64Ds, Uptr};
use wasixv2_host::Host;
use wasixv2_pages::SharedPages;
use wasixv2_syscalls::{Caller, Errno, SysResult, read_value, write_value};
use wasixv2_vfs::Cap;

use super::{Ids, Object, Perm, READ, WRITE, get_or_create};
use crate::{
    Kernel,
    mm::{self, Mapped, PAGE_SIZE, Shared, Source},
    ns::UserNs,
    sync::MutexExt,
};

/// `SHMMIN`, `SHMMNI` and `SHMMAX`/`SHMALL` for a 32-bit `unsigned long`.
pub(crate) const SHMMIN: u32 = 1;
pub(crate) const SHMMNI: u32 = 4096;
pub(crate) const SHMMAX: u32 = u32::MAX - (1 << 24);
pub(crate) const SHMALL: u32 = SHMMAX;
/// `SHM_DEST` and `SHM_LOCKED` in `shm_perm.mode`.
const SHM_DEST: u32 = 0o1000;
const SHM_LOCKED: u32 = 0o2000;
const PAGE: usize = PAGE_SIZE as usize;

/// The namespace's segments.
#[derive(Default)]
pub(crate) struct Segments {
    ids: Mutex<Ids<Segment>>,
    /// Pages of all segments (`shm_tot`).
    total: Mutex<u64>,
}

pub(crate) struct Segment {
    state: Mutex<State>,
    perm: Mutex<Perm>,
    pages: Backing,
    size: usize,
}

/// Where a segment's bytes live.
enum Backing {
    /// A host shared memory object of the segment's whole pages, which attachments map.
    Host(Arc<dyn SharedPages>),
    /// Kernel memory, for hosts without shared memory objects; attachments are copies.
    Heap(Mutex<Pages>),
}

/// A segment's bytes, by page; pages never written read as zeroes.
#[derive(Default)]
struct Pages {
    map: HashMap<usize, Box<[u8]>>,
    /// Counts the writes, so an attachment can tell whether anybody else wrote since it last
    /// looked.
    generation: u64,
}

impl Object for Segment {
    fn perm(&self) -> MutexGuard<'_, Perm> {
        self.perm.lock_or_recover()
    }
}

struct State {
    /// Destroyed: no longer in the table, attachable by nobody.
    gone: bool,
    nattch: u32,
    cpid: u32,
    lpid: u32,
    atime: u64,
    dtime: u64,
    ctime: u64,
}

impl Segment {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock_or_recover()
    }

    fn page_count(&self) -> u64 {
        self.size.div_ceil(PAGE) as u64
    }

    /// The host object that holds the segment, which attachments map.
    fn host_pages(&self) -> Option<Arc<dyn SharedPages>> {
        match &self.pages {
            Backing::Host(pages) => Some(pages.clone()),
            Backing::Heap(_) => None,
        }
    }

    /// Pages of memory the segment holds now (`shm_rss`).
    fn resident(&self) -> u64 {
        match &self.pages {
            Backing::Host(pages) => pages.allocated().div_ceil(PAGE_SIZE),
            Backing::Heap(pages) => pages.lock_or_recover().map.len() as u64,
        }
    }

    /// Copies out `buf.len()` bytes from `offset`; past the segment's pages they read as zero.
    /// Returns how many were inside, and the generation they are of, if the segment counts its
    /// writes: one in a host object does not, as attachments that map it write it directly.
    pub(crate) fn read(&self, offset: u64, buf: &mut [u8]) -> Result<(usize, Option<u64>), Errno> {
        let end = self.page_count() as usize * PAGE;
        let inside = end.saturating_sub(offset as usize).min(buf.len());
        let pages = match &self.pages {
            Backing::Host(pages) => {
                pages
                    .read_at(&mut buf[..inside], offset)
                    .map_err(io_errno)?;
                buf[inside..].fill(0);
                return Ok((inside, None));
            }
            Backing::Heap(pages) => pages.lock_or_recover(),
        };
        let (mut at, mut done) = (offset as usize, 0);
        while done < inside {
            let n = (PAGE - at % PAGE).min(inside - done);
            let to = &mut buf[done..][..n];
            match pages.map.get(&(at / PAGE)) {
                Some(page) => to.copy_from_slice(&page[at % PAGE..][..n]),
                None => to.fill(0),
            }
            (at, done) = (at + n, done + n);
        }
        buf[inside..].fill(0);
        Ok((inside, Some(pages.generation)))
    }

    /// Copies `data` in at `offset`, up to the end of the segment's pages. Returns the
    /// generation the segment has now, if it counts its writes.
    pub(crate) fn write(&self, offset: u64, data: &[u8]) -> Result<Option<u64>, Errno> {
        let end = self.page_count() as usize * PAGE;
        let mut at = offset as usize;
        let mut rest = &data[..end.saturating_sub(at).min(data.len())];
        let mut pages = match &self.pages {
            Backing::Host(pages) => {
                pages.write_at(rest, offset).map_err(io_errno)?;
                return Ok(None);
            }
            Backing::Heap(pages) => pages.lock_or_recover(),
        };
        while !rest.is_empty() {
            let page = pages
                .map
                .entry(at / PAGE)
                .or_insert_with(|| vec![0; PAGE].into_boxed_slice());
            let n = (PAGE - at % PAGE).min(rest.len());
            page[at % PAGE..][..n].copy_from_slice(&rest[..n]);
            at += n;
            rest = &rest[n..];
        }
        pages.generation += 1;
        Ok(Some(pages.generation))
    }

    /// The segment's generation, how many writes it had, if it counts them.
    pub(crate) fn generation(&self) -> Option<u64> {
        match &self.pages {
            Backing::Host(_) => None,
            Backing::Heap(pages) => Some(pages.lock_or_recover().generation),
        }
    }

    fn to_abi(&self, perm: &Perm, ns: &UserNs) -> Shmid64Ds {
        let state = self.state();
        let (atime, atime_high) = super::split_time(state.atime);
        let (dtime, dtime_high) = super::split_time(state.dtime);
        let (ctime, ctime_high) = super::split_time(state.ctime);
        Shmid64Ds {
            shm_perm: perm.to_abi(ns),
            shm_segsz: self.size as u32,
            shm_atime: atime,
            shm_atime_high: atime_high,
            shm_dtime: dtime,
            shm_dtime_high: dtime_high,
            shm_ctime: ctime,
            shm_ctime_high: ctime_high,
            shm_cpid: state.cpid as i32,
            shm_lpid: state.lpid as i32,
            shm_nattch: state.nattch,
            ..Shmid64Ds::default()
        }
    }
}

/// One `shmat`: holds the segment attached until every part of the mapping is gone
/// (`shm_open`/`shm_close`).
pub(crate) struct Attachment {
    segment: Arc<Segment>,
    id: i32,
    table: Arc<Segments>,
    pid: u32,
    host: Arc<dyn Host>,
}

impl Attachment {
    pub(crate) fn segment(&self) -> &Segment {
        &self.segment
    }

    /// The same attachment in the fork child `pid` (`shm_open` from `dup_mmap`).
    pub(crate) fn dup(&self, pid: u32) -> Arc<Attachment> {
        {
            let mut state = self.segment.state();
            state.nattch += 1;
            state.atime = self.host.realtime().as_secs();
            state.lpid = self.pid;
        }
        Arc::new(Attachment {
            segment: self.segment.clone(),
            id: self.id,
            table: self.table.clone(),
            pid,
            host: self.host.clone(),
        })
    }
}

impl Drop for Attachment {
    /// `shm_close`: the last detach of a segment marked for removal destroys it.
    fn drop(&mut self) {
        let destroy = {
            let mut state = self.segment.state();
            state.nattch -= 1;
            state.lpid = self.pid;
            state.dtime = self.host.realtime().as_secs();
            state.nattch == 0 && self.segment.perm().mode & SHM_DEST != 0
        };
        if destroy {
            self.table.destroy(self.id, &self.segment);
        }
    }
}

impl Segments {
    fn get(&self, id: i32) -> Result<Arc<Segment>, Errno> {
        self.ids.lock_or_recover().get(id).ok_or(Errno::EINVAL)
    }

    /// `shm_destroy`.
    fn destroy(&self, id: i32, segment: &Segment) {
        let mut ids = self.ids.lock_or_recover();
        let mut state = segment.state();
        if state.gone {
            return;
        }
        state.gone = true;
        if ids.get(id).is_some_and(|s| std::ptr::eq(&*s, segment)) {
            ids.remove(id);
        }
        *self.total.lock_or_recover() -= segment.page_count();
    }

    /// Rows of `/proc/sysvipc/shm`.
    #[allow(
        dead_code,
        reason = "the provider for /proc/sysvipc/shm, for procfs to read"
    )]
    pub(crate) fn list(&self) -> Vec<ShmInfo> {
        self.ids
            .lock_or_recover()
            .all()
            .into_iter()
            .map(|(id, segment)| {
                let state = segment.state();
                ShmInfo {
                    id,
                    perm: segment.perm().clone(),
                    size: segment.size,
                    cpid: state.cpid,
                    lpid: state.lpid,
                    nattch: state.nattch,
                    atime: state.atime,
                    dtime: state.dtime,
                    ctime: state.ctime,
                }
            })
            .collect()
    }
}

/// One segment, as `/proc/sysvipc/shm` shows it.
#[allow(dead_code, reason = "read by procfs")]
pub(crate) struct ShmInfo {
    pub(crate) id: i32,
    pub(crate) perm: Perm,
    pub(crate) size: usize,
    pub(crate) cpid: u32,
    pub(crate) lpid: u32,
    pub(crate) nattch: u32,
    pub(crate) atime: u64,
    pub(crate) dtime: u64,
    pub(crate) ctime: u64,
}

impl Kernel {
    pub(crate) fn sys_shmget(
        &self,
        caller: &Caller<'_>,
        key: i32,
        size: u32,
        shmflg: i32,
    ) -> SysResult<i32> {
        let cred = self.ipc_cred(caller);
        let now = self.ipc_now();
        let pid = self.process(caller.task()).pid.0;
        let ns = self.ns(caller);
        let segments = &ns.ipc.shm;
        let check = |segment: &Segment| match segment.size < size as usize {
            true => Err(Errno::EINVAL),
            false => Ok(()),
        };
        let create = |perm| {
            if !(SHMMIN..=SHMMAX).contains(&size) {
                return Err(Errno::EINVAL);
            }
            let pages = u64::from(size).div_ceil(PAGE_SIZE);
            let mut total = segments.total.lock_or_recover();
            if *total + pages > u64::from(SHMALL) {
                return Err(Errno::ENOSPC);
            }
            let backing = self.segment_backing(pages)?;
            *total += pages;
            Ok(Segment {
                state: Mutex::new(State {
                    gone: false,
                    nattch: 0,
                    cpid: pid,
                    lpid: 0,
                    atime: 0,
                    dtime: 0,
                    ctime: now,
                }),
                perm: Mutex::new(perm),
                pages: backing,
                size: size as usize,
            })
        };
        let limit = SHMMNI as usize;
        Ok(get_or_create(
            &segments.ids,
            key,
            shmflg,
            &cred,
            limit,
            check,
            create,
        )?)
    }

    pub(crate) async fn sys_shmat(
        &self,
        caller: &Caller<'_>,
        shmid: i32,
        shmaddr: Uptr,
        shmflg: i32,
    ) -> SysResult<i32> {
        let flags = ShmFlags(shmflg as u32);
        if shmid < 0 {
            return Err(Errno::EINVAL.into());
        }
        // SHMLBA is the page size.
        let lba = PAGE_SIZE as u32;
        let mut addr = shmaddr;
        if addr != 0 {
            if !addr.is_multiple_of(lba) {
                if !flags.contains(ShmFlags::SHM_RND) {
                    return Err(Errno::EINVAL.into());
                }
                addr -= addr % lba;
                if addr == 0 && flags.contains(ShmFlags::SHM_REMAP) {
                    return Err(Errno::EINVAL.into());
                }
            }
        } else if flags.contains(ShmFlags::SHM_REMAP) {
            return Err(Errno::EINVAL.into());
        }
        let readonly = flags.contains(ShmFlags::SHM_RDONLY);
        let mut access = match readonly {
            true => READ,
            false => READ | WRITE,
        };
        if flags.contains(ShmFlags::SHM_EXEC) {
            access |= 0o111;
        }
        let cred = self.ipc_cred(caller);
        let pid = self.process(caller.task()).pid.0;
        let segment = self.ns(caller).ipc.shm.get(shmid)?;
        {
            let mut state = segment.state();
            segment.perm().check(&cred, access)?;
            if state.gone {
                return Err(Errno::EIDRM.into());
            }
            if flags.contains(ShmFlags::SHM_EXEC) {
                // Wasm cannot run code from memory (`PROT_EXEC`).
                return Err(Errno::EACCES.into());
            }
            state.nattch += 1;
        }
        let attachment = Arc::new(Attachment {
            segment: segment.clone(),
            id: shmid,
            table: self.ns(caller).ipc.shm.clone(),
            pid,
            host: self.host.clone(),
        });
        let source = match segment.host_pages() {
            Some(pages) => Source::Mapped(Mapped {
                pages,
                offset: 0,
                segment: Some(attachment),
                writable: !readonly,
                writer: None,
            }),
            None => {
                let object = mm::Object::Segment(attachment);
                Source::Shared(Shared::new(object, 0, !readonly, None))
            }
        };
        let len = segment.size as u64;
        let remap = flags.contains(ShmFlags::SHM_REMAP);
        let at = self.map_segment(caller, addr, len, remap, source).await?;
        let mut state = segment.state();
        state.atime = self.ipc_now();
        state.lpid = pid;
        Ok(at as i32)
    }

    pub(crate) async fn sys_shmdt(&self, caller: &Caller<'_>, shmaddr: Uptr) -> SysResult<i32> {
        if u64::from(shmaddr) % PAGE_SIZE != 0 {
            return Err(Errno::EINVAL.into());
        }
        self.unmap_segment(caller, shmaddr).await?;
        Ok(0)
    }

    pub(crate) async fn sys_shmctl(
        &self,
        caller: &Caller<'_>,
        shmid: i32,
        cmd: i32,
        buf: u32,
    ) -> SysResult<i32> {
        if cmd < 0 || shmid < 0 {
            return Err(Errno::EINVAL.into());
        }
        let cred = self.ipc_cred(caller);
        let ns = self.ns(caller);
        let segments = &ns.ipc.shm;
        match IpcCmd(cmd) {
            IpcCmd::IPC_INFO => {
                let max = segments.ids.lock_or_recover().max_index();
                let info = [SHMMAX, SHMMIN, SHMMNI, SHMMNI, SHMALL, 0, 0, 0, 0];
                write_words(caller, buf, &info).await?;
                Ok(max)
            }
            IpcCmd::SHM_INFO => {
                let (used, max, rss) = {
                    let ids = segments.ids.lock_or_recover();
                    let all = ids.all();
                    let rss: u64 = all.iter().map(|(_, s)| s.resident()).sum();
                    (ids.in_use() as u32, ids.max_index(), rss)
                };
                let total = *segments.total.lock_or_recover() as u32;
                let info = [used, total, rss as u32, 0, 0, 0];
                write_words(caller, buf, &info).await?;
                Ok(max)
            }
            IpcCmd::IPC_STAT | IpcCmd::SHM_STAT | super::SHM_STAT_ANY => {
                let (id, segment) = match IpcCmd(cmd) {
                    IpcCmd::IPC_STAT => (shmid, segments.get(shmid)?),
                    _ => {
                        let segment = segments
                            .ids
                            .lock_or_recover()
                            .at(shmid)
                            .ok_or(Errno::EINVAL)?;
                        let id = super::build_id(shmid as usize, segment.perm().seq);
                        (id, segment)
                    }
                };
                let perm = segment.perm().clone();
                if IpcCmd(cmd) != super::SHM_STAT_ANY {
                    perm.check(&cred, READ)?;
                }
                let ds = segment.to_abi(&perm, &self.cred(caller).user_ns);
                // The creator and last user as the caller sees them (`pid_vnr`).
                let ds = Shmid64Ds {
                    shm_cpid: self.vnr(caller, ds.shm_cpid as u32),
                    shm_lpid: self.vnr(caller, ds.shm_lpid as u32),
                    ..ds
                };
                write_value(caller.memory(), GuestPtrMut::new(buf), &ds).await?;
                Ok(match IpcCmd(cmd) {
                    IpcCmd::IPC_STAT => 0,
                    _ => id,
                })
            }
            IpcCmd::IPC_SET => {
                let ds: Shmid64Ds = read_value(caller.memory(), GuestPtr::new(buf)).await?;
                let segment = segments.get(shmid)?;
                let mut state = segment.state();
                let mut perm = segment.perm();
                perm.check_owner(&cred)?;
                perm.update(&ds.shm_perm, &self.cred(caller).user_ns)?;
                state.ctime = self.ipc_now();
                Ok(0)
            }
            IpcCmd::IPC_RMID => {
                let mut ids = segments.ids.lock_or_recover();
                let segment = ids.get(shmid).ok_or(Errno::EINVAL)?;
                segment.perm().check_owner(&cred)?;
                // `do_shm_rmid`: attached segments only lose their key until the last detach.
                let attached = segment.state().nattch > 0;
                if attached {
                    segment.perm().mode |= SHM_DEST;
                    ids.forget_key(shmid, &segment);
                    return Ok(0);
                }
                drop(ids);
                segments.destroy(shmid, &segment);
                Ok(0)
            }
            IpcCmd::SHM_LOCK | IpcCmd::SHM_UNLOCK => {
                let segment = segments.get(shmid)?;
                let state = segment.state();
                if state.gone {
                    return Err(Errno::EIDRM.into());
                }
                let mut perm = segment.perm();
                if !cred.ns_capable(Cap::IpcLock) {
                    if cred.fsuid != perm.uid && cred.fsuid != perm.cuid {
                        return Err(Errno::EPERM.into());
                    }
                    let memlock = self
                        .process(caller.task())
                        .resources
                        .lock_or_recover()
                        .soft(RlimitResource::RLIMIT_MEMLOCK);
                    if IpcCmd(cmd) == IpcCmd::SHM_LOCK && memlock == 0 {
                        return Err(Errno::EPERM.into());
                    }
                }
                match IpcCmd(cmd) {
                    IpcCmd::SHM_LOCK => perm.mode |= SHM_LOCKED,
                    _ => perm.mode &= !SHM_LOCKED,
                }
                Ok(0)
            }
            _ => Err(Errno::EINVAL.into()),
        }
    }
}

impl Kernel {
    /// Where a new segment of `pages` pages keeps its bytes: a host shared memory object
    /// (`shmem_kernel_file_setup`), or kernel memory where the host has none.
    fn segment_backing(&self, pages: u64) -> Result<Backing, Errno> {
        match self.host.shared_pages(pages * PAGE_SIZE) {
            Ok(pages) => Ok(Backing::Host(pages)),
            Err(err) if err.kind() == io::ErrorKind::Unsupported => {
                Ok(Backing::Heap(Mutex::default()))
            }
            Err(_) => Err(Errno::ENOMEM),
        }
    }
}

/// The errno for a host shared memory object that failed a read or write.
fn io_errno(_: io::Error) -> Errno {
    Errno::ENOMEM
}

/// Copies an `*info` structure of `unsigned long`s out to `addr`.
async fn write_words(caller: &Caller<'_>, addr: u32, words: &[u32]) -> SysResult<()> {
    let bytes: Vec<u8> = words.iter().flat_map(|v| v.to_le_bytes()).collect();
    caller.memory().write(addr, &bytes).await?;
    Ok(())
}
