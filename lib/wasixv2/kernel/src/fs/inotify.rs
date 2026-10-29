//! inotify (`fs/notify/inotify`): instances are descriptors with an event queue; watches are
//! keyed by object identity and fed by the VFS's `FsWatcher` hook.

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use wasixv2_abi::{
    AtFd, AtFlags, Cstr, GuestPtrMut, InotifyInitFlags, InotifyMask, IoctlRequest, OpenFlags,
};
use wasixv2_syscalls::{Caller, Errno, Fd, LocalFuture, SysErr, SysResult, write_value};
use wasixv2_vfs::{Dentry, FileType, FsEvent, FsObject, FsWatcher, MAY_READ};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait, wait_event};

use super::read_path;
use crate::{
    Kernel,
    fasync::{Fasync, FasyncList},
    file::{Backing, FileObject, Io, OpenFile},
    sync::MutexExt,
    userns::{Ucount, UcountKind},
};

const IN_ISDIR: u32 = 0x4000_0000;
const IN_ONESHOT: u32 = 0x8000_0000;
const IN_MASK_ADD: u32 = 0x2000_0000;
const IN_EXCL_UNLINK: u32 = 0x0400_0000;
const IN_MASK_CREATE: u32 = 0x1000_0000;
/// `ALL_INOTIFY_BITS`: what a watch's mask may hold.
const ALL_BITS: u32 = InotifyMask::IN_ALL_EVENTS.0
    | InotifyMask::IN_UNMOUNT.0
    | InotifyMask::IN_Q_OVERFLOW.0
    | InotifyMask::IN_IGNORED.0
    | InotifyMask::IN_ONLYDIR.0
    | InotifyMask::IN_DONT_FOLLOW.0
    | IN_EXCL_UNLINK
    | IN_MASK_ADD
    | IN_MASK_CREATE
    | IN_ISDIR
    | IN_ONESHOT;
/// `struct inotify_event` without its name.
const EVENT_SIZE: usize = 16;

/// The watches on one object: instance and watch descriptor.
type Watchers = Vec<(Weak<Instance>, i32)>;

/// Every watch of the machine, by watched object.
#[derive(Default)]
pub(crate) struct Registry {
    watches: Mutex<HashMap<FsObject, Watchers>>,
    /// Whether `watches` holds anything, readable without its lock.
    watched: AtomicBool,
    cookie: AtomicU32,
}

impl Registry {
    /// Changes the watch table, keeping `watched` in step.
    fn update<R>(&self, f: impl FnOnce(&mut HashMap<FsObject, Watchers>) -> R) -> R {
        let mut watches = self.watches.lock_or_recover();
        let result = f(&mut watches);
        self.watched.store(!watches.is_empty(), Ordering::Release);
        result
    }

    /// Queues `mask` for every watch on `object` that wants it. An event about an object
    /// whose name was removed (`unlinked`) skips `IN_EXCL_UNLINK` watches.
    fn deliver(&self, object: FsObject, mask: u32, cookie: u32, name: &[u8], unlinked: bool) {
        let targets: Vec<(Arc<Instance>, i32)> = {
            let watches = self.watches.lock_or_recover();
            let Some(list) = watches.get(&object) else {
                return;
            };
            list.iter()
                .filter_map(|(inotify, wd)| Some((inotify.upgrade()?, *wd)))
                .collect()
        };
        for (inotify, wd) in &targets {
            inotify.event(self, *wd, mask, cookie, name, unlinked);
        }
    }

    /// The object is gone: `IN_DELETE_SELF`, then `IN_IGNORED` as its watches go away.
    fn gone(&self, object: FsObject) {
        self.deliver(object, InotifyMask::IN_DELETE_SELF.0, 0, b"", false);
        let removed = self.update(|watches| watches.remove(&object));
        for (inotify, wd) in removed.into_iter().flatten() {
            if let Some(inotify) = inotify.upgrade() {
                inotify.forget(wd);
                inotify.push(wd, InotifyMask::IN_IGNORED.0, 0, b"");
            }
        }
    }

    /// An object event for the object's own watches.
    fn unlinked_or_not(&self, event: &FsEvent<'_>, unlinked: bool) {
        if let Some((object, mask)) = object_event(event) {
            self.deliver(object, mask, 0, b"", unlinked);
        }
    }

    fn unwatch(&self, object: FsObject, inotify: &Instance, wd: i32) {
        self.update(|watches| {
            if let Some(list) = watches.get_mut(&object) {
                list.retain(|(i, w)| !(*w == wd && std::ptr::eq(i.as_ptr(), inotify)));
                if list.is_empty() {
                    watches.remove(&object);
                }
            }
        });
    }
}

impl FsWatcher for Registry {
    fn watching(&self) -> bool {
        self.watched.load(Ordering::Acquire)
    }

    fn watches_sb(&self, sb: u64) -> bool {
        self.watching() && self.watches.lock_or_recover().keys().any(|o| o.sb == sb)
    }

    fn event(&self, event: &FsEvent<'_>) {
        let isdir = |is_dir: bool| if is_dir { IN_ISDIR } else { 0 };
        match *event {
            FsEvent::Create { dir, name, is_dir } => self.deliver(
                dir,
                InotifyMask::IN_CREATE.0 | isdir(is_dir),
                0,
                name,
                false,
            ),
            FsEvent::Delete {
                dir,
                name,
                is_dir,
                object,
                gone,
            } => {
                if !gone {
                    self.deliver(object, InotifyMask::IN_ATTRIB.0, 0, b"", false);
                }
                self.deliver(
                    dir,
                    InotifyMask::IN_DELETE.0 | isdir(is_dir),
                    0,
                    name,
                    false,
                );
                if gone {
                    self.gone(object);
                }
            }
            FsEvent::Move {
                from_dir,
                from_name,
                to_dir,
                to_name,
                is_dir,
                object,
            } => {
                let cookie = self.cookie.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
                let flag = isdir(is_dir);
                self.deliver(
                    from_dir,
                    InotifyMask::IN_MOVED_FROM.0 | flag,
                    cookie,
                    from_name,
                    false,
                );
                self.deliver(
                    to_dir,
                    InotifyMask::IN_MOVED_TO.0 | flag,
                    cookie,
                    to_name,
                    false,
                );
                self.deliver(object, InotifyMask::IN_MOVE_SELF.0, 0, b"", false);
            }
            _ => self.unlinked_or_not(event, false),
        }
    }

    fn unlinked_event(&self, event: &FsEvent<'_>) {
        self.unlinked_or_not(event, true);
    }

    fn child_event(&self, dir: FsObject, name: &[u8], event: &FsEvent<'_>, unlinked: bool) {
        if let Some((_, mask)) = object_event(event) {
            self.deliver(dir, mask, 0, name, unlinked);
        }
    }
}

fn object_event(event: &FsEvent<'_>) -> Option<(FsObject, u32)> {
    Some(match *event {
        FsEvent::Modify { object } => (object, InotifyMask::IN_MODIFY.0),
        FsEvent::Attrib { object } => (object, InotifyMask::IN_ATTRIB.0),
        FsEvent::Access { object } => (object, InotifyMask::IN_ACCESS.0),
        FsEvent::Open { object } => (object, InotifyMask::IN_OPEN.0),
        FsEvent::Close { object, wrote } => (
            object,
            match wrote {
                true => InotifyMask::IN_CLOSE_WRITE.0,
                false => InotifyMask::IN_CLOSE_NOWRITE.0,
            },
        ),
        _ => return None,
    })
}

struct Watch {
    object: FsObject,
    mask: u32,
    /// The watched inode, held as Linux's mark holds it (`fsnotify_add_inode_mark`), so its
    /// number stays its own while the watch lasts. Not the mount: a watch does not keep a
    /// filesystem from being unmounted.
    _pin: Arc<Dentry>,
    /// Counted against `max_inotify_watches` of the instance's creator (`inc_inotify_watches`).
    _ucount: Ucount,
}

#[derive(Default)]
struct State {
    queue: VecDeque<Vec<u8>>,
    /// Bytes queued, for `FIONREAD`.
    queued: usize,
    watches: HashMap<i32, Watch>,
    last_wd: i32,
    overflowed: bool,
}

/// One inotify instance: the open file's object. Dropping it (the last close) removes its
/// watches.
pub(crate) struct Inotify {
    inner: Arc<Instance>,
    registry: Arc<Registry>,
}

/// What watches point to.
struct Instance {
    /// The most events its queue holds (`max_events`), past which it overflows.
    max_events: usize,
    /// Counted against `max_inotify_instances` of its creator, whose watches count against
    /// `max_inotify_watches` (`inotify_data.ucounts`).
    ucount: Ucount,
    state: Mutex<State>,
    wait: WaitQueue,
    /// The files that get I/O signals (`fsn_fa`).
    fasync: FasyncList,
}

impl Instance {
    fn event(
        &self,
        registry: &Registry,
        wd: i32,
        mask: u32,
        cookie: u32,
        name: &[u8],
        unlinked: bool,
    ) {
        let wanted = {
            let mut state = self.state.lock_or_recover();
            let Some(watch) = state.watches.get(&wd) else {
                return;
            };
            if watch.mask & mask & InotifyMask::IN_ALL_EVENTS.0 == 0 {
                return;
            }
            // `fsnotify_handle_inode_event`: `IN_EXCL_UNLINK` skips objects with no name.
            if unlinked && watch.mask & IN_EXCL_UNLINK != 0 {
                return;
            }
            let oneshot = watch.mask & IN_ONESHOT != 0;
            let object = watch.object;
            if oneshot {
                state.watches.remove(&wd);
            }
            oneshot.then_some(object)
        };
        self.push(wd, mask, cookie, name);
        if let Some(object) = wanted {
            registry.unwatch(object, self, wd);
            self.push(wd, InotifyMask::IN_IGNORED.0, 0, b"");
        }
    }

    fn forget(&self, wd: i32) {
        self.state.lock_or_recover().watches.remove(&wd);
    }

    /// Queues one event, merging it into an identical last one (`inotify_merge`).
    fn push(&self, wd: i32, mask: u32, cookie: u32, name: &[u8]) {
        let len = match name.len() {
            0 => 0,
            n => (n + 1).next_multiple_of(EVENT_SIZE),
        };
        let mut record = Vec::with_capacity(EVENT_SIZE + len);
        record.extend_from_slice(&wd.to_ne_bytes());
        record.extend_from_slice(&mask.to_ne_bytes());
        record.extend_from_slice(&cookie.to_ne_bytes());
        record.extend_from_slice(&(len as u32).to_ne_bytes());
        record.extend_from_slice(name);
        record.resize(EVENT_SIZE + len, 0);
        {
            let mut state = self.state.lock_or_recover();
            if state.queue.back() == Some(&record) {
                return;
            }
            if state.queue.len() >= self.max_events {
                if state.overflowed {
                    return;
                }
                state.overflowed = true;
                record = overflow_record();
            }
            state.queued += record.len();
            state.queue.push_back(record);
        }
        self.wait.wake(PollMask::READABLE);
        self.fasync.kill(wasixv2_abi::SiCode::POLL_IN);
    }

    /// `inotify_read`: whole events only; `EINVAL` if the first does not fit.
    async fn read_events(&self, io: Io<'_>, buf: &mut [u8]) -> SysResult<usize> {
        let take = || {
            let mut state = self.state.lock_or_recover();
            let Some(first) = state.queue.front() else {
                return io.nonblocking().then_some(Err(Errno::EAGAIN));
            };
            if first.len() > buf.len() {
                return Some(Err(Errno::EINVAL));
            }
            let mut n = 0;
            while let Some(event) = state.queue.front() {
                if n + event.len() > buf.len() {
                    break;
                }
                let event = state.queue.pop_front().unwrap();
                buf[n..n + event.len()].copy_from_slice(&event);
                n += event.len();
                state.queued -= event.len();
            }
            if state.queue.is_empty() {
                state.overflowed = false;
            }
            Some(Ok(n))
        };
        let result = wait_event(&self.wait, take, io.interrupt)
            .await
            .map_err(|_| SysErr::Restart)?;
        Ok(result?)
    }
}

fn overflow_record() -> Vec<u8> {
    let mut record = vec![0; EVENT_SIZE];
    record[..4].copy_from_slice(&(-1i32).to_ne_bytes());
    record[4..8].copy_from_slice(&InotifyMask::IN_Q_OVERFLOW.0.to_ne_bytes());
    record
}

impl Drop for Inotify {
    fn drop(&mut self) {
        let watches = std::mem::take(&mut self.inner.state.lock_or_recover().watches);
        self.registry.update(|registry| {
            for watch in watches.values() {
                if let Some(list) = registry.get_mut(&watch.object) {
                    list.retain(|(i, _)| !std::ptr::eq(i.as_ptr(), &*self.inner));
                    if list.is_empty() {
                        registry.remove(&watch.object);
                    }
                }
            }
        });
    }
}

impl Pollable for Inotify {
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.inner.wait);
        match self.inner.state.lock_or_recover().queue.is_empty() {
            true => PollMask::empty(),
            false => PollMask::READABLE,
        }
    }
}

impl FileObject for Inotify {
    fn read<'a>(&'a self, io: Io<'a>, buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(self.inner.read_events(io, buf))
    }

    fn write<'a>(&'a self, _: Io<'a>, _: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EINVAL.into()) })
    }

    /// `fsnotify_fasync`.
    fn fasync(&self, fasync: &Fasync<'_>) -> Option<Result<(), Errno>> {
        self.inner.fasync.helper(fasync);
        Some(Ok(()))
    }

    fn ioctl<'a>(
        &'a self,
        caller: &'a Caller<'a>,
        request: IoctlRequest,
        arg: u32,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async move {
            if request != IoctlRequest::FIONREAD {
                return Err(Errno::ENOTTY.into());
            }
            let queued = self.inner.state.lock_or_recover().queued as i32;
            write_value(caller.memory(), GuestPtrMut::new(arg), &queued).await?;
            Ok(0)
        })
    }
}

impl Kernel {
    pub(crate) fn sys_inotify_init1(
        &self,
        caller: &Caller<'_>,
        flags: InotifyInitFlags,
    ) -> SysResult<Fd> {
        let known = InotifyInitFlags::IN_NONBLOCK | InotifyInitFlags::IN_CLOEXEC;
        if !(flags & !known).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        // `inotify_new_group`: the limit is what `fs.inotify.max_queued_events` says now.
        let max_events = self
            .tunables
            .get(crate::sysctl::Tunable::InotifyMaxQueuedEvents);
        // `inc_ucount(current_user_ns(), current_euid(), UCOUNT_INOTIFY_INSTANCES)`.
        let cred = self.process(caller.task()).cred();
        let ucount = Ucount::charge(
            &cred.user_ns,
            cred.user.effective,
            UcountKind::InotifyInstances,
        )
        .map_err(|_| Errno::EMFILE)?;
        let inotify = Inotify {
            inner: Arc::new(Instance {
                max_events: max_events as usize,
                ucount,
                state: Mutex::default(),
                wait: WaitQueue::new(),
                fasync: FasyncList::default(),
            }),
            registry: self.inotify.clone(),
        };
        let mut status = OpenFlags::O_RDONLY;
        if flags.contains(InotifyInitFlags::IN_NONBLOCK) {
            status |= OpenFlags::O_NONBLOCK;
        }
        let file = OpenFile::new(
            Backing::Stream(Arc::new(inotify), self.pseudo.anon("inotify")),
            status,
        );
        let cloexec = flags.contains(InotifyInitFlags::IN_CLOEXEC);
        self.install(caller, Arc::new(file), cloexec)
    }

    pub(crate) async fn sys_inotify_add_watch(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        pathname: Cstr,
        mask: InotifyMask,
    ) -> SysResult<i32> {
        // Only inotify's bits, and at least one of them: a mask of flags alone is a watch.
        let mask = mask.0;
        if mask & !ALL_BITS != 0 || mask & ALL_BITS == 0 {
            return Err(Errno::EINVAL.into());
        }
        if mask & IN_MASK_ADD != 0 && mask & IN_MASK_CREATE != 0 {
            return Err(Errno::EINVAL.into());
        }
        let file = self.file(caller.task(), fd)?;
        let inotify = file
            .downcast::<Inotify>()
            .ok_or(Errno::EINVAL)?
            .inner
            .clone();
        let path = read_path(caller.memory(), pathname).await?;
        let env = self.env(caller);
        let follow = mask & InotifyMask::IN_DONT_FOLLOW.0 == 0;
        let at = wasixv2_abi::Fd(AtFd::AT_FDCWD.0);
        let target = self
            .resolve_at(caller, &env, at, &path, AtFlags(0), follow)
            .await?;
        if mask & InotifyMask::IN_ONLYDIR.0 != 0 && target.dentry.file_type() != FileType::Directory
        {
            return Err(Errno::ENOTDIR.into());
        }
        self.vfs.permission(&env.cx(), &target, MAY_READ).await?;
        let object = FsObject {
            sb: target.dentry.sb().id,
            ino: target.dentry.node().ino(),
        };
        let mut state = inotify.state.lock_or_recover();
        let existing = state
            .watches
            .iter()
            .find(|(_, watch)| watch.object == object)
            .map(|(wd, _)| *wd);
        if let Some(wd) = existing {
            if mask & IN_MASK_CREATE != 0 {
                return Err(Errno::EEXIST.into());
            }
            let watch = state.watches.get_mut(&wd).unwrap();
            watch.mask = match mask & IN_MASK_ADD {
                0 => mask,
                _ => watch.mask | mask,
            };
            return Ok(wd);
        }
        // `inotify_new_watch`: ENOSPC past `max_inotify_watches`.
        let ucount = inotify.ucount.charge_another(UcountKind::InotifyWatches)?;
        state.last_wd += 1;
        let wd = state.last_wd;
        let watch = Watch {
            object,
            mask,
            _pin: target.dentry,
            _ucount: ucount,
        };
        state.watches.insert(wd, watch);
        drop(state);
        self.inotify.update(|watches| {
            let list = watches.entry(object).or_default();
            list.push((Arc::downgrade(&inotify), wd));
        });
        Ok(wd)
    }

    pub(crate) fn sys_inotify_rm_watch(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        wd: i32,
    ) -> SysResult<i32> {
        let file = self.file(caller.task(), fd)?;
        let inotify = &file.downcast::<Inotify>().ok_or(Errno::EINVAL)?.inner;
        let watch = inotify
            .state
            .lock_or_recover()
            .watches
            .remove(&wd)
            .ok_or(Errno::EINVAL)?;
        self.inotify.unwatch(watch.object, inotify, wd);
        inotify.push(wd, InotifyMask::IN_IGNORED.0, 0, b"");
        Ok(0)
    }
}
