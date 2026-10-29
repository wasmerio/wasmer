//! dnotify (`fs/notify/dnotify/dnotify.c`): `F_NOTIFY` on a directory's open file makes the
//! file's owner hear of changes to the directory and the objects in it, by `SIGIO` or the
//! file's `F_SETSIG` signal with `POLL_MSG` and the descriptor in `si_fd`. It hangs off the
//! VFS's change notifications, as inotify does ([`Notify`]).

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

use wasixv2_abi::{DnotifyFlags, InotifyMask, SiCode};
use wasixv2_syscalls::{Errno, Fd, SysResult, TaskId};
use wasixv2_vfs::{FileType, FsEvent, FsObject, FsWatcher, Vfs};

use super::inotify::Registry;
use crate::{
    Kernel,
    fasync::{OwnerType, Sigio},
    file::{Backing, OpenFile},
    sync::MutexExt,
};

/// The `fsnotify` events dnotify asks for (`FS_*`, which inotify's numbers are).
const FS_ACCESS: u32 = InotifyMask::IN_ACCESS.0;
const FS_MODIFY: u32 = InotifyMask::IN_MODIFY.0;
const FS_ATTRIB: u32 = InotifyMask::IN_ATTRIB.0;
const FS_MOVED_FROM: u32 = InotifyMask::IN_MOVED_FROM.0;
const FS_MOVED_TO: u32 = InotifyMask::IN_MOVED_TO.0;
const FS_CREATE: u32 = InotifyMask::IN_CREATE.0;
const FS_DELETE: u32 = InotifyMask::IN_DELETE.0;
/// A rename within one directory, which only dnotify takes.
const FS_RENAME: u32 = 0x1000_0000;

/// `convert_arg`: the events a `DN_*` mask stands for. A file moved out of the directory is
/// deleted from it, one moved in is created there.
fn events(arg: DnotifyFlags) -> u32 {
    let table = [
        (DnotifyFlags::DN_DELETE, FS_DELETE | FS_MOVED_FROM),
        (DnotifyFlags::DN_MODIFY, FS_MODIFY),
        (DnotifyFlags::DN_ACCESS, FS_ACCESS),
        (DnotifyFlags::DN_ATTRIB, FS_ATTRIB),
        (DnotifyFlags::DN_RENAME, FS_RENAME),
        (DnotifyFlags::DN_CREATE, FS_CREATE | FS_MOVED_TO),
    ];
    table
        .iter()
        .filter(|(dn, _)| arg.intersects(*dn))
        .fold(0, |mask, (_, fs)| mask | fs)
}

/// One registration (`dnotify_struct`): an open file of the directory, for the processes of
/// one descriptor table, which `F_NOTIFY` named by `fd`.
struct Entry {
    file: Weak<OpenFile>,
    fd: Fd,
    /// `dn_owner`: whose descriptor it is, as POSIX locks name their owners.
    owner: TaskId,
    mask: u32,
    multishot: bool,
}

/// Every dnotify registration of the machine, by watched directory.
pub(crate) struct Dnotify {
    marks: Mutex<HashMap<FsObject, Vec<Entry>>>,
    /// Whether `marks` holds anything, readable without its lock.
    watched: AtomicBool,
    sigio: Arc<Sigio>,
}

impl Dnotify {
    pub(crate) fn new(sigio: Arc<Sigio>) -> Self {
        Self {
            marks: Mutex::default(),
            watched: AtomicBool::new(false),
            sigio,
        }
    }

    fn update<R>(&self, f: impl FnOnce(&mut HashMap<FsObject, Vec<Entry>>) -> R) -> R {
        let mut marks = self.marks.lock_or_recover();
        let result = f(&mut marks);
        self.watched.store(!marks.is_empty(), Ordering::Release);
        result
    }

    /// `attach_dn`: registers `file`, open as `fd` in `owner`'s table, for `mask`, or adds
    /// `mask` to its registration and makes it name `fd`.
    fn attach(
        &self,
        object: FsObject,
        file: &Arc<OpenFile>,
        fd: Fd,
        owner: TaskId,
        arg: DnotifyFlags,
    ) {
        let mask = events(arg);
        let multishot = arg.contains(DnotifyFlags::DN_MULTISHOT);
        self.update(|marks| {
            let list = marks.entry(object).or_default();
            let mine = list
                .iter_mut()
                .find(|e| e.owner == owner && std::ptr::eq(e.file.as_ptr(), &**file));
            match mine {
                Some(entry) => {
                    entry.fd = fd;
                    entry.mask |= mask;
                    entry.multishot |= multishot;
                }
                None => list.push(Entry {
                    file: Arc::downgrade(file),
                    fd,
                    owner,
                    mask,
                    multishot,
                }),
            }
        });
    }

    /// `dnotify_flush`: `owner` closed a descriptor of `file`, or asked for nothing.
    pub(crate) fn flush(&self, object: FsObject, file: &OpenFile, owner: TaskId) {
        if !self.watched.load(Ordering::Acquire) {
            return;
        }
        self.update(|marks| {
            let Some(list) = marks.get_mut(&object) else {
                return;
            };
            list.retain(|e| !(e.owner == owner && std::ptr::eq(e.file.as_ptr(), file)));
            if list.is_empty() {
                marks.remove(&object);
            }
        });
    }

    /// `dnotify_handle_event`: `mask` happened to `object` or in it. Each registration that
    /// asked for it signals its file's owner; those without `DN_MULTISHOT` go.
    fn deliver(&self, object: FsObject, mask: u32) {
        let targets: Vec<(Arc<OpenFile>, Fd)> = self.update(|marks| {
            let Some(list) = marks.get_mut(&object) else {
                return Vec::new();
            };
            let mut targets = Vec::new();
            list.retain(|e| {
                if e.mask & mask == 0 {
                    return true;
                }
                let Some(file) = e.file.upgrade() else {
                    return false;
                };
                targets.push((file, e.fd));
                e.multishot
            });
            if list.is_empty() {
                marks.remove(&object);
            }
            targets
        });
        for (file, fd) in targets {
            self.sigio.send(&file, fd, SiCode::POLL_MSG);
        }
    }

    fn object_event(&self, event: &FsEvent<'_>) {
        if let Some((object, mask)) = object_event(event) {
            self.deliver(object, mask);
        }
    }
}

/// An event an object has of itself, which reaches a directory's registrations when the
/// object is the directory, and its parent's (with `child_event`) when it is in one.
fn object_event(event: &FsEvent<'_>) -> Option<(FsObject, u32)> {
    match *event {
        FsEvent::Modify { object } => Some((object, FS_MODIFY)),
        FsEvent::Attrib { object } => Some((object, FS_ATTRIB)),
        FsEvent::Access { object } => Some((object, FS_ACCESS)),
        _ => None,
    }
}

impl FsWatcher for Dnotify {
    fn watching(&self) -> bool {
        self.watched.load(Ordering::Acquire)
    }

    fn watches_sb(&self, sb: u64) -> bool {
        self.watching() && self.marks.lock_or_recover().keys().any(|o| o.sb == sb)
    }

    /// `fsnotify_create`, `fsnotify_delete` with `fsnotify_link_count`, `fsnotify_move`
    /// (`FS_RENAME` only reaches a directory a name moved within, `fsnotify_handle_event`)
    /// and the object events.
    fn event(&self, event: &FsEvent<'_>) {
        match *event {
            FsEvent::Create { dir, .. } => self.deliver(dir, FS_CREATE),
            FsEvent::Delete {
                dir, object, gone, ..
            } => {
                if !gone {
                    self.deliver(object, FS_ATTRIB);
                }
                self.deliver(dir, FS_DELETE);
            }
            FsEvent::Move {
                from_dir, to_dir, ..
            } => {
                if from_dir == to_dir {
                    self.deliver(from_dir, FS_RENAME);
                }
                self.deliver(from_dir, FS_MOVED_FROM);
                self.deliver(to_dir, FS_MOVED_TO);
            }
            _ => self.object_event(event),
        }
    }

    fn child_event(&self, dir: FsObject, _: &[u8], event: &FsEvent<'_>, _: bool) {
        if let Some((_, mask)) = object_event(event) {
            self.deliver(dir, mask);
        }
    }
}

/// The VFS's one watcher: inotify's, dnotify's and fanotify's registrations all hear every
/// change (`fsnotify`'s groups).
pub(crate) struct Notify {
    inotify: Arc<Registry>,
    dnotify: Arc<Dnotify>,
    fanotify: Arc<super::fanotify::Registry>,
}

impl Notify {
    /// Feeds `inotify`, `dnotify` and `fanotify` from `vfs`'s change notifications.
    pub(crate) fn install(
        vfs: &Vfs,
        inotify: Arc<Registry>,
        dnotify: Arc<Dnotify>,
        fanotify: Arc<super::fanotify::Registry>,
    ) {
        vfs.set_watcher(Arc::new(Self {
            inotify,
            dnotify,
            fanotify,
        }));
    }
}

impl FsWatcher for Notify {
    fn pre_content_watch(&self, path: &wasixv2_vfs::Path) -> bool {
        self.fanotify.pre_content_watch(path)
    }

    fn watching(&self) -> bool {
        self.inotify.watching() || self.dnotify.watching() || self.fanotify.watching()
    }

    fn mount_event(&self, event: wasixv2_vfs::MountEvent) {
        self.fanotify.mount_event(event);
    }

    fn mount_namespace_deleted(&self, namespace: u64) {
        self.fanotify.mount_namespace_deleted(namespace);
    }

    fn fsnotify(&self, event: &FsEvent<'_>, info: &wasixv2_vfs::EventInfo<'_>) {
        self.fanotify.fsnotify(event, info);
    }

    fn permission(
        &self,
        path: &wasixv2_vfs::Path,
        perm: wasixv2_vfs::Perm,
    ) -> Option<wasixv2_vfs::PermAnswer> {
        self.fanotify.permission(path, perm)
    }

    fn watches_sb(&self, sb: u64) -> bool {
        self.inotify.watches_sb(sb) || self.dnotify.watches_sb(sb)
    }

    fn event(&self, event: &FsEvent<'_>) {
        self.inotify.event(event);
        self.dnotify.event(event);
    }

    fn unlinked_event(&self, event: &FsEvent<'_>) {
        self.inotify.unlinked_event(event);
        self.dnotify.unlinked_event(event);
    }

    fn child_event(&self, dir: FsObject, name: &[u8], event: &FsEvent<'_>, unlinked: bool) {
        self.inotify.child_event(dir, name, event, unlinked);
        self.dnotify.child_event(dir, name, event, unlinked);
    }
}

impl Kernel {
    /// `fcntl_dirnotify`: `F_NOTIFY` on `file`, open as `fd`. An empty mask (`DN_MULTISHOT`
    /// aside) takes the caller's registration away; anything else needs a directory, and
    /// makes the caller's process the file's owner unless it has one.
    pub(crate) fn fcntl_dirnotify(
        &self,
        caller: &wasixv2_syscalls::Caller<'_>,
        fd: Fd,
        file: &Arc<OpenFile>,
        arg: u32,
    ) -> SysResult<i32> {
        if self.sysctls.dir_notify_enable() == 0 {
            return Err(Errno::EINVAL.into());
        }
        let arg = DnotifyFlags(arg);
        let owner = self.process(caller.task()).pid;
        let Backing::Inode(inner) = &file.backing else {
            // Not an inode: nothing could be registered, and it is no directory.
            return match (arg.0 & !DnotifyFlags::DN_MULTISHOT.0) == 0 {
                true => Ok(0),
                false => Err(Errno::ENOTDIR.into()),
            };
        };
        let (sb, ino) = inner.lock_key();
        let object = FsObject { sb, ino };
        if (arg.0 & !DnotifyFlags::DN_MULTISHOT.0) == 0 {
            self.dnotify.flush(object, file, owner);
            return Ok(0);
        }
        if inner.file_type() != FileType::Directory {
            return Err(Errno::ENOTDIR.into());
        }
        // `__f_setown(filp, task_pid(current), PIDTYPE_TGID, 0)`.
        let cred = self.cred(caller);
        let uids = (cred.user.real, cred.user.effective);
        file.set_owner(Some(owner.0), OwnerType::Process, uids, false);
        self.dnotify.attach(object, file, fd, owner, arg);
        Ok(0)
    }
}
