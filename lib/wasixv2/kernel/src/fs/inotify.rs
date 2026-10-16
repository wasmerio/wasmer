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
use wasixv2_vfs::{FileType, FsEvent, FsObject, FsWatcher, MAY_READ, Vfs};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait, wait_event};

use super::read_path;
use crate::{
    Kernel,
    file::{Backing, FileObject, Io, OpenFile},
};

const IN_ISDIR: u32 = 0x4000_0000;
const IN_ONESHOT: u32 = 0x8000_0000;
const IN_MASK_ADD: u32 = 0x2000_0000;
const IN_EXCL_UNLINK: u32 = 0x0400_0000;
const IN_MASK_CREATE: u32 = 0x1000_0000;
/// `ALL_INOTIFY_BITS`: what a watch's mask may hold.
const ALL_BITS: u32 = InotifyMask::IN_ALL_EVENTS.0
    | InotifyMask::IN_ONLYDIR.0
    | InotifyMask::IN_DONT_FOLLOW.0
    | IN_EXCL_UNLINK
    | IN_MASK_ADD
    | IN_MASK_CREATE
    | IN_ISDIR
    | IN_ONESHOT;
/// `struct inotify_event` without its name.
const EVENT_SIZE: usize = 16;
/// `/proc/sys/fs/inotify/max_queued_events`.
const MAX_QUEUED: usize = 16384;

/// The watches on one object: instance and watch descriptor.
type Watchers = Vec<(Weak<Instance>, i32)>;

/// Every watch of the machine, by watched object.
#[derive(Default)]
pub(crate) struct Registry {
    watches: Mutex<HashMap<FsObject, Watchers>>,
    /// `watches` is not empty, so the VFS's event fast path can tell without the lock.
    watching: AtomicBool,
    cookie: AtomicU32,
}

impl Registry {
    /// A registry fed by `vfs`'s change notifications.
    pub(crate) fn install(vfs: &Vfs) -> Arc<Self> {
        let registry = Arc::new(Self::default());
        vfs.set_watcher(Some(registry.clone()));
        registry
    }

    /// Queues `mask` for every watch on `object` that wants it.
    fn deliver(&self, object: FsObject, mask: u32, cookie: u32, name: &[u8]) {
        let targets: Vec<(Arc<Instance>, i32)> = {
            let watches = self.watches.lock().unwrap();
            let Some(list) = watches.get(&object) else {
                return;
            };
            list.iter()
                .filter_map(|(inotify, wd)| Some((inotify.upgrade()?, *wd)))
                .collect()
        };
        for (inotify, wd) in &targets {
            inotify.event(self, *wd, mask, cookie, name);
        }
    }

    /// Records whether anything is watched, after a change to `watches`.
    fn changed(&self, watches: &HashMap<FsObject, Watchers>) {
        self.watching.store(!watches.is_empty(), Ordering::Release);
    }

    /// The object is gone: `IN_DELETE_SELF`, then `IN_IGNORED` as its watches go away.
    fn gone(&self, object: FsObject) {
        self.deliver(object, InotifyMask::IN_DELETE_SELF.0, 0, b"");
        let removed = {
            let mut watches = self.watches.lock().unwrap();
            let removed = watches.remove(&object);
            self.changed(&watches);
            removed
        };
        for (inotify, wd) in removed.into_iter().flatten() {
            if let Some(inotify) = inotify.upgrade() {
                inotify.forget(wd);
                inotify.push(wd, InotifyMask::IN_IGNORED.0, 0, b"");
            }
        }
    }

    fn unwatch(&self, object: FsObject, inotify: &Instance, wd: i32) {
        let mut watches = self.watches.lock().unwrap();
        if let Some(list) = watches.get_mut(&object) {
            list.retain(|(i, w)| !(*w == wd && std::ptr::eq(i.as_ptr(), inotify)));
            if list.is_empty() {
                watches.remove(&object);
            }
        }
        self.changed(&watches);
    }
}

impl FsWatcher for Registry {
    fn is_watching(&self) -> bool {
        self.watching.load(Ordering::Acquire)
    }

    fn event(&self, event: &FsEvent<'_>) {
        let isdir = |is_dir: bool| if is_dir { IN_ISDIR } else { 0 };
        match *event {
            FsEvent::Create { dir, name, is_dir } => {
                self.deliver(dir, InotifyMask::IN_CREATE.0 | isdir(is_dir), 0, name)
            }
            FsEvent::Delete {
                dir,
                name,
                is_dir,
                object,
                gone,
            } => {
                if !gone {
                    self.deliver(object, InotifyMask::IN_ATTRIB.0, 0, b"");
                }
                self.deliver(dir, InotifyMask::IN_DELETE.0 | isdir(is_dir), 0, name);
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
                );
                self.deliver(to_dir, InotifyMask::IN_MOVED_TO.0 | flag, cookie, to_name);
                self.deliver(object, InotifyMask::IN_MOVE_SELF.0, 0, b"");
            }
            _ => {
                if let Some((object, mask)) = object_event(event) {
                    self.deliver(object, mask, 0, b"");
                }
            }
        }
    }

    fn child_event(&self, dir: FsObject, name: &[u8], event: &FsEvent<'_>) {
        if let Some((_, mask)) = object_event(event) {
            self.deliver(dir, mask, 0, name);
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
    state: Mutex<State>,
    wait: WaitQueue,
}

impl Instance {
    fn event(&self, registry: &Registry, wd: i32, mask: u32, cookie: u32, name: &[u8]) {
        let wanted = {
            let mut state = self.state.lock().unwrap();
            let Some(watch) = state.watches.get(&wd) else {
                return;
            };
            if watch.mask & mask & InotifyMask::IN_ALL_EVENTS.0 == 0 {
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
        self.state.lock().unwrap().watches.remove(&wd);
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
            let mut state = self.state.lock().unwrap();
            if state.queue.back() == Some(&record) {
                return;
            }
            if state.queue.len() >= MAX_QUEUED {
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
    }

    /// `inotify_read`: whole events only; `EINVAL` if the first does not fit.
    async fn read_events(&self, io: Io<'_>, buf: &mut [u8]) -> SysResult<usize> {
        let take = || {
            let mut state = self.state.lock().unwrap();
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
        let watches = std::mem::take(&mut self.inner.state.lock().unwrap().watches);
        let mut registry = self.registry.watches.lock().unwrap();
        for watch in watches.values() {
            if let Some(list) = registry.get_mut(&watch.object) {
                list.retain(|(i, _)| !std::ptr::eq(i.as_ptr(), &*self.inner));
                if list.is_empty() {
                    registry.remove(&watch.object);
                }
            }
        }
        self.registry.changed(&registry);
    }
}

impl Pollable for Inotify {
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.inner.wait);
        match self.inner.state.lock().unwrap().queue.is_empty() {
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
            let queued = self.inner.state.lock().unwrap().queued as i32;
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
        let inotify = Inotify {
            inner: Arc::new(Instance {
                state: Mutex::default(),
                wait: WaitQueue::new(),
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
        let mask = mask.0;
        if mask & ALL_BITS == 0 || mask & InotifyMask::IN_ALL_EVENTS.0 == 0 {
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
        self.vfs.permission(&env.cred, &target, MAY_READ).await?;
        let object = FsObject {
            sb: target.dentry.sb().id,
            ino: target.dentry.node().ino(),
        };
        let mut state = inotify.state.lock().unwrap();
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
        state.last_wd += 1;
        let wd = state.last_wd;
        state.watches.insert(wd, Watch { object, mask });
        drop(state);
        let mut watches = self.inotify.watches.lock().unwrap();
        watches
            .entry(object)
            .or_default()
            .push((Arc::downgrade(&inotify), wd));
        self.inotify.changed(&watches);
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
            .lock()
            .unwrap()
            .watches
            .remove(&wd)
            .ok_or(Errno::EINVAL)?;
        self.inotify.unwatch(watch.object, inotify, wd);
        inotify.push(wd, InotifyMask::IN_IGNORED.0, 0, b"");
        Ok(0)
    }
}
