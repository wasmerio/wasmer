//! fanotify (`fs/notify/fanotify`): groups are descriptors with an event queue and marks on
//! inodes, mounts and filesystems, fed by the VFS's `FsWatcher` hook as inotify is. A group of
//! a permission class answers opens and reads before they happen.
//!
//! Not provided: filesystem
//! error events (accepted on filesystem marks, never generated) and auditing.

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

use wasixv2_abi::{
    Cstr, FanotifyInitFlags, FanotifyMarkFlags, FanotifyMask, FanotifyMisc, GuestPtrMut,
    IoctlRequest, OpenFlags, SiCode,
};
use wasixv2_syscalls::{
    Caller, Errno, Fd, LocalFuture, SysErr, SysResult, current_task, write_value,
};
use wasixv2_vfs::{
    Cap, Dentry, EventInfo, FileId, FileType, FsEvent, FsObject, MAY_READ, Path, Perm, PermAnswer,
    Vfs,
};
use wasixv2_wait::{NoInterrupt, PollMask, PollTable, Pollable, WaitQueue, poll_wait, wait_event};

use super::read_path;
use crate::{
    Kernel, TaskTable,
    fasync::{Fasync, FasyncList},
    file::{Backing, FileObject, Io, OpenFile},
    sync::MutexExt,
};

const fn bit(mask: FanotifyMask) -> u32 {
    mask.0 as u32
}

const ACCESS: u32 = bit(FanotifyMask::FAN_ACCESS);
const MODIFY: u32 = bit(FanotifyMask::FAN_MODIFY);
const ATTRIB: u32 = bit(FanotifyMask::FAN_ATTRIB);
const CLOSE_WRITE: u32 = bit(FanotifyMask::FAN_CLOSE_WRITE);
const CLOSE_NOWRITE: u32 = bit(FanotifyMask::FAN_CLOSE_NOWRITE);
const OPEN: u32 = bit(FanotifyMask::FAN_OPEN);
const MOVED_FROM: u32 = bit(FanotifyMask::FAN_MOVED_FROM);
const MOVED_TO: u32 = bit(FanotifyMask::FAN_MOVED_TO);
const CREATE: u32 = bit(FanotifyMask::FAN_CREATE);
const DELETE: u32 = bit(FanotifyMask::FAN_DELETE);
const DELETE_SELF: u32 = bit(FanotifyMask::FAN_DELETE_SELF);
const MOVE_SELF: u32 = bit(FanotifyMask::FAN_MOVE_SELF);
const OPEN_EXEC: u32 = bit(FanotifyMask::FAN_OPEN_EXEC);
const Q_OVERFLOW: u32 = bit(FanotifyMask::FAN_Q_OVERFLOW);
const FS_ERROR: u32 = bit(FanotifyMask::FAN_FS_ERROR);
const OPEN_PERM: u32 = bit(FanotifyMask::FAN_OPEN_PERM);
const ACCESS_PERM: u32 = bit(FanotifyMask::FAN_ACCESS_PERM);
const OPEN_EXEC_PERM: u32 = bit(FanotifyMask::FAN_OPEN_EXEC_PERM);
const PRE_ACCESS: u32 = bit(FanotifyMask::FAN_PRE_ACCESS);
const MNT_EVENTS: u32 = bit(FanotifyMask::FAN_MNT_ATTACH) | bit(FanotifyMask::FAN_MNT_DETACH);
const ON_CHILD: u32 = bit(FanotifyMask::FAN_EVENT_ON_CHILD);
const RENAME: u32 = bit(FanotifyMask::FAN_RENAME);
const ONDIR: u32 = bit(FanotifyMask::FAN_ONDIR);

/// `FANOTIFY_PATH_EVENTS`.
const PATH_EVENTS: u32 = ACCESS | MODIFY | CLOSE_WRITE | CLOSE_NOWRITE | OPEN | OPEN_EXEC;
/// `FANOTIFY_DIRENT_EVENTS`: reported to the directory whose entries changed.
const DIRENT_EVENTS: u32 = MOVED_FROM | MOVED_TO | CREATE | DELETE | RENAME;
/// `FANOTIFY_INODE_EVENTS`.
const INODE_EVENTS: u32 = DIRENT_EVENTS | ATTRIB | MOVE_SELF | DELETE_SELF;
/// `FANOTIFY_CONTENT_PERM_EVENTS`.
const PERM_EVENTS: u32 = OPEN_PERM | OPEN_EXEC_PERM | ACCESS_PERM;
/// `FANOTIFY_EVENTS`.
const EVENTS: u32 = PATH_EVENTS | INODE_EVENTS | FS_ERROR | MNT_EVENTS;
/// `FANOTIFY_EVENT_FLAGS`.
const EVENT_FLAGS: u32 = ON_CHILD | ONDIR;
/// `FANOTIFY_OUTGOING_EVENTS`.
const OUTGOING: u32 = EVENTS | PERM_EVENTS | PRE_ACCESS | Q_OVERFLOW | ONDIR;
/// `FANOTIFY_DIRONLY_EVENT_BITS`.
const DIRONLY: u32 = DIRENT_EVENTS | ON_CHILD | ONDIR;
/// `FANOTIFY_FD_EVENTS`.
const FD_EVENTS: u32 = PATH_EVENTS | PERM_EVENTS | PRE_ACCESS;
/// `ALL_FSNOTIFY_EVENTS` as far as fanotify uses it: every event bit, no flag.
const ALL_EVENTS: u32 = EVENTS | PERM_EVENTS | PRE_ACCESS | Q_OVERFLOW;

const fn init(flags: FanotifyInitFlags) -> u32 {
    flags.0
}

const CLASS_CONTENT: u32 = init(FanotifyInitFlags::FAN_CLASS_CONTENT);
const CLASS_PRE_CONTENT: u32 = init(FanotifyInitFlags::FAN_CLASS_PRE_CONTENT);
/// `FANOTIFY_CLASS_BITS`.
const CLASS_BITS: u32 = CLASS_CONTENT | CLASS_PRE_CONTENT;
const REPORT_TID: u32 = init(FanotifyInitFlags::FAN_REPORT_TID);
const REPORT_PIDFD: u32 = init(FanotifyInitFlags::FAN_REPORT_PIDFD);
const REPORT_FID: u32 = init(FanotifyInitFlags::FAN_REPORT_FID);
const REPORT_DIR_FID: u32 = init(FanotifyInitFlags::FAN_REPORT_DIR_FID);
const REPORT_NAME: u32 = init(FanotifyInitFlags::FAN_REPORT_NAME);
const REPORT_TARGET_FID: u32 = init(FanotifyInitFlags::FAN_REPORT_TARGET_FID);
const REPORT_FD_ERROR: u32 = init(FanotifyInitFlags::FAN_REPORT_FD_ERROR);
const REPORT_MNT: u32 = init(FanotifyInitFlags::FAN_REPORT_MNT);
const UNLIMITED_QUEUE: u32 = init(FanotifyInitFlags::FAN_UNLIMITED_QUEUE);
const UNLIMITED_MARKS: u32 = init(FanotifyInitFlags::FAN_UNLIMITED_MARKS);
/// `FANOTIFY_FID_BITS`.
const FID_BITS: u32 = REPORT_FID | REPORT_DIR_FID | REPORT_NAME | REPORT_TARGET_FID;
/// `FANOTIFY_ADMIN_INIT_FLAGS`.
const ADMIN_INIT: u32 =
    CLASS_BITS | REPORT_TID | REPORT_PIDFD | REPORT_FD_ERROR | UNLIMITED_QUEUE | UNLIMITED_MARKS;
/// `FANOTIFY_USER_INIT_FLAGS`.
const USER_INIT: u32 = FID_BITS
    | REPORT_MNT
    | init(FanotifyInitFlags::FAN_CLOEXEC)
    | init(FanotifyInitFlags::FAN_NONBLOCK);
/// `FANOTIFY_UNPRIV`: a group made without `CAP_SYS_ADMIN`.
const UNPRIV: u32 = 0x8000_0000;
/// `FANOTIFY_INIT_ALL_EVENT_F_BITS`.
const EVENT_F_BITS: u32 = OpenFlags::O_ACCMODE.0
    | OpenFlags::O_APPEND.0
    | OpenFlags::O_NONBLOCK.0
    | OpenFlags::__O_SYNC.0
    | OpenFlags::O_DSYNC.0
    | OpenFlags::O_CLOEXEC.0
    | OpenFlags::O_LARGEFILE.0
    | OpenFlags::O_NOATIME.0;

const fn mark(flags: FanotifyMarkFlags) -> u32 {
    flags.0
}

const MARK_ADD: u32 = mark(FanotifyMarkFlags::FAN_MARK_ADD);
const MARK_REMOVE: u32 = mark(FanotifyMarkFlags::FAN_MARK_REMOVE);
const MARK_FLUSH: u32 = mark(FanotifyMarkFlags::FAN_MARK_FLUSH);
const MARK_DONT_FOLLOW: u32 = mark(FanotifyMarkFlags::FAN_MARK_DONT_FOLLOW);
const MARK_ONLYDIR: u32 = mark(FanotifyMarkFlags::FAN_MARK_ONLYDIR);
const MARK_MOUNT: u32 = mark(FanotifyMarkFlags::FAN_MARK_MOUNT);
const MARK_FILESYSTEM: u32 = mark(FanotifyMarkFlags::FAN_MARK_FILESYSTEM);
const MARK_MNTNS: u32 = mark(FanotifyMarkFlags::FAN_MARK_MNTNS);
const MARK_IGNORED_MASK: u32 = mark(FanotifyMarkFlags::FAN_MARK_IGNORED_MASK);
const MARK_IGNORE: u32 = mark(FanotifyMarkFlags::FAN_MARK_IGNORE);
const MARK_SURV_MODIFY: u32 = mark(FanotifyMarkFlags::FAN_MARK_IGNORED_SURV_MODIFY);
const MARK_EVICTABLE: u32 = mark(FanotifyMarkFlags::FAN_MARK_EVICTABLE);
/// `FANOTIFY_MARK_TYPE_BITS`.
const MARK_TYPE_BITS: u32 = MARK_MOUNT | MARK_FILESYSTEM | MARK_MNTNS;
/// `FANOTIFY_MARK_IGNORE_BITS`.
const MARK_IGNORE_BITS: u32 = MARK_IGNORED_MASK | MARK_IGNORE;
/// `FANOTIFY_MARK_FLAGS`.
const MARK_FLAGS: u32 = MARK_TYPE_BITS
    | MARK_ADD
    | MARK_REMOVE
    | MARK_FLUSH
    | MARK_IGNORE_BITS
    | MARK_DONT_FOLLOW
    | MARK_ONLYDIR
    | MARK_SURV_MODIFY
    | MARK_EVICTABLE;

/// `FAN_ALLOW`, `FAN_DENY`, `FAN_AUDIT` and `FAN_INFO`.
const ALLOW: u32 = FanotifyMisc::FAN_ALLOW.0 as u32;
const DENY: u32 = FanotifyMisc::FAN_DENY.0 as u32;
const AUDIT: u32 = FanotifyMisc::FAN_AUDIT.0 as u32;
const INFO: u32 = FanotifyMisc::FAN_INFO.0 as u32;
const ERRNO_SHIFT: u32 = FanotifyMisc::FAN_ERRNO_SHIFT.0 as u32;
const ERRNO_MASK: u32 = FanotifyMisc::FAN_ERRNO_MASK.0 as u32;
/// `FANOTIFY_RESPONSE_VALID_MASK`.
const RESPONSE_VALID: u32 = ALLOW | DENY | AUDIT | INFO | (ERRNO_MASK << ERRNO_SHIFT);
const NOFD: i32 = FanotifyMisc::FAN_NOFD.0;

/// `struct fanotify_event_metadata`, `struct fanotify_event_info_fid` with the
/// `struct file_handle` header after it, and `struct fanotify_event_info_pidfd`.
const METADATA_LEN: usize = 24;
const FID_INFO_HDR_LEN: usize = 20;
const PIDFD_INFO_LEN: usize = 8;
/// `FANOTIFY_DEFAULT_MAX_EVENTS`, `FANOTIFY_DEFAULT_MAX_GROUPS` and
/// `FANOTIFY_DEFAULT_MAX_USER_MARKS`.
const MAX_EVENTS: usize = 16384;
const MAX_GROUPS: usize = 128;
const MAX_MARKS: usize = 8192 * 128;
/// `FANOTIFY_MAX_MERGE_EVENTS`: how far back a new event looks for one to merge into.
const MAX_MERGE: usize = 128;

/// What a mark is on (`FSNOTIFY_OBJ_TYPE_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Target {
    Inode(FsObject),
    /// A mount, by id.
    Mount(u64),
    /// A filesystem, by superblock id.
    Sb(u64),
    /// Mount namespace's VFS id and nsfs inode number.
    Namespace(u64, u32),
}

/// How a mark matched an event (`FSNOTIFY_ITER_TYPE_*`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Iter {
    Inode,
    /// The directory the object is in, which hears of it with `FAN_EVENT_ON_CHILD`.
    Parent,
    /// The new directory of a rename.
    Inode2,
    Mount,
    Sb,
}

struct Mark {
    target: Target,
    mask: u32,
    ignore: u32,
    /// `FSNOTIFY_MARK_FLAG_IGNORED_SURV_MODIFY`.
    surv_modify: bool,
    /// `FSNOTIFY_MARK_FLAG_HAS_IGNORE_FLAGS`: the ignore mask's event flags count
    /// (`FAN_MARK_IGNORE`).
    ignore_flags: bool,
    /// `FSNOTIFY_MARK_FLAG_NO_IREF`.
    evictable: bool,
    /// The marked inode, held as an inode mark holds it unless it is evictable.
    _pin: Option<Arc<Dentry>>,
}

impl Mark {
    /// `fsnotify_ignore_mask`.
    fn ignore_mask(&self) -> u32 {
        if self.ignore_flags {
            return self.ignore;
        }
        (self.ignore | ONDIR) & !ON_CHILD | (self.mask & ON_CHILD)
    }

    /// `fsnotify_effective_ignore_mask`.
    fn effective_ignore(&self, is_dir: bool, iter: Iter) -> u32 {
        let events = self.ignore & ALL_EVENTS;
        if events == 0 {
            return 0;
        }
        if !is_dir && iter != Iter::Parent {
            return events;
        }
        let ignore = self.ignore_mask();
        if !applicable(ignore, is_dir, iter) {
            return 0;
        }
        ignore & ALL_EVENTS
    }
}

/// `fsnotify_mask_applicable`.
fn applicable(mask: u32, is_dir: bool, iter: Iter) -> bool {
    let dir_ok = !is_dir || mask & ONDIR != 0;
    let child_ok = iter != Iter::Parent || mask & ON_CHILD != 0;
    dir_ok && child_ok
}

/// A file handle as an information record reports it.
#[derive(Clone, PartialEq, Eq)]
struct Fh {
    fid: FileId,
}

/// What an event reports besides its mask.
#[derive(Clone)]
enum Info {
    /// The object's path, from which the reader gets a descriptor.
    Path {
        path: Path,
        range: Option<(u64, u64)>,
    },
    /// File identifiers (`fanotify_fid_event`, `fanotify_name_event`).
    Fid {
        fsid: [i32; 2],
        /// The directory and the entry's name (empty for none).
        dir: Option<(Fh, Vec<u8>)>,
        /// The new directory and name of a rename.
        dir2: Option<(Fh, Vec<u8>)>,
        object: Option<Fh>,
    },
    Mount(u64),
    Overflow,
}

/// A permission event's progress (`FAN_EVENT_INIT`, `REPORTED`, `ANSWERED`).
struct PermState {
    state: Mutex<PermProgress>,
    answered: WaitQueue,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PermProgress {
    Queued,
    /// Read, as the descriptor `fd` of the reader.
    Reported(i32),
    Answered(u32),
}

impl PermState {
    fn answer(&self, response: u32) {
        *self.state.lock_or_recover() = PermProgress::Answered(response);
        self.answered.wake(PollMask::READABLE);
    }
}

struct Event {
    mask: u32,
    /// The task that caused it and its process, as kernel ids; 0 for none.
    tid: u32,
    tgid: u32,
    pid: Option<crate::pid::PidRef>,
    info: Info,
    perm: Option<Arc<PermState>>,
}

impl Event {
    /// `fanotify_event_len`.
    fn len(&self, flags: u32) -> usize {
        let mut len = METADATA_LEN;
        if let Info::Fid {
            dir, dir2, object, ..
        } = &self.info
        {
            let fh_len = |fh: &Fh, name: usize| {
                let body = fh.fid.data.len() + if name > 0 { name + 1 } else { 0 };
                (FID_INFO_HDR_LEN + body).next_multiple_of(4)
            };
            let mut dot = 0;
            if dir.is_some() || dir2.is_some() {
                for (fh, name) in [dir, dir2].into_iter().flatten() {
                    len += fh_len(fh, name.len());
                }
            } else if flags & REPORT_NAME != 0 && self.mask & ONDIR != 0 {
                dot = 1;
            }
            if let Some(fh) = object {
                len += fh_len(fh, dot);
            }
        }
        if matches!(self.info, Info::Path { range: Some(_), .. }) {
            len += 24;
        }
        if matches!(self.info, Info::Mount(_)) {
            len += 16;
        }
        if flags & REPORT_PIDFD != 0 {
            len += PIDFD_INFO_LEN;
        }
        len
    }

    /// `fanotify_should_merge`.
    fn merges_with(&self, new: &Event) -> bool {
        if (self.tid, self.tgid) != (new.tid, new.tgid) || self.perm.is_some() {
            return false;
        }
        if (self.mask ^ new.mask) & (ONDIR | RENAME) != 0 {
            return false;
        }
        match (&self.info, &new.info) {
            (Info::Path { path: a, range: ar }, Info::Path { path: b, range: br }) => {
                a == b && ar == br
            }
            (Info::Mount(a), Info::Mount(b)) => a == b,
            (
                Info::Fid {
                    fsid,
                    dir,
                    dir2,
                    object,
                },
                Info::Fid {
                    fsid: fsid2,
                    dir: dir_b,
                    dir2: dir2_b,
                    object: object_b,
                },
            ) => fsid == fsid2 && dir == dir_b && dir2 == dir2_b && object == object_b,
            _ => false,
        }
    }
}

#[derive(Default)]
struct GroupState {
    queue: VecDeque<Event>,
    /// The overflow event is queued.
    overflowed: bool,
    marks: Vec<Mark>,
    /// Permission events read and not answered yet (`access_list`).
    access: Vec<Arc<PermState>>,
    /// The group's descriptor is closed: no event joins it any more.
    shut: bool,
}

/// One fanotify group: the open file's object.
pub(crate) struct Group {
    /// `fanotify_init`'s flags, with `UNPRIV`.
    flags: u32,
    /// The status flags of the descriptors its events open (`event_f_flags`).
    f_flags: OpenFlags,
    /// 0 for notification, 1 for content and 2 for pre-content (`FSNOTIFY_PRIO_*`).
    priority: u8,
    /// Who made it: its effective uid, whose group and mark counts it takes, and the user
    /// namespace it was made in (`group->user_ns`).
    owner: u32,
    user_ns: Arc<crate::ns::UserNs>,
    max_events: usize,
    state: Mutex<GroupState>,
    wait: WaitQueue,
    fasync: FasyncList,
    registry: Arc<Registry>,
}

/// Every group with marks, and the counts the limits are kept by.
#[derive(Default)]
pub(crate) struct Registry {
    groups: Mutex<Vec<Weak<Group>>>,
    /// Whether any group has a mark, readable without the lock.
    marked: AtomicBool,
    /// Groups and marks per effective uid (`UCOUNT_FANOTIFY_GROUPS`, `_MARKS`).
    counts: Mutex<HashMap<u32, (usize, usize)>>,
    tasks: std::sync::OnceLock<Weak<TaskTable>>,
    vfs: std::sync::OnceLock<Weak<Vfs>>,
}

/// One change in fsnotify's terms: the event bits (with `FAN_ONDIR`), the object it is
/// about, the directory and name it came through, and for data access the path.
struct Change<'a> {
    mask: u32,
    object: FsObject,
    path: Option<&'a Path>,
    /// The directory and name: of the entry for a directory entry event, else of the object
    /// in its parent.
    dir: Option<(FsObject, &'a [u8])>,
    /// The new directory and name of a rename (`FS_RENAME`).
    moved_to: Option<(FsObject, &'a [u8])>,
    /// An event about the object only, which its parent does not hear of
    /// (`fsnotify_inode`).
    self_only: bool,
    range: Option<(u64, u64)>,
}

impl Registry {
    /// Lets the registry find who causes events and encode the objects of events.
    pub(crate) fn bind(&self, tasks: &Arc<TaskTable>, vfs: &Arc<Vfs>) {
        let _ = self.tasks.set(Arc::downgrade(tasks));
        let _ = self.vfs.set(Arc::downgrade(vfs));
    }

    /// Whether any group has a mark (`FsWatcher::watching`).
    pub(crate) fn watching(&self) -> bool {
        self.marked.load(Ordering::Acquire)
    }

    fn groups(&self) -> Vec<Arc<Group>> {
        let mut groups = self.groups.lock_or_recover();
        groups.retain(|group| group.strong_count() > 0);
        groups.iter().filter_map(Weak::upgrade).collect()
    }

    fn recount(&self) {
        let marked = self
            .groups()
            .iter()
            .any(|group| !group.state.lock_or_recover().marks.is_empty());
        self.marked.store(marked, Ordering::Release);
    }

    /// The current task and its process, as kernel ids.
    fn current(&self) -> (u32, u32) {
        let Some(task) = current_task() else {
            return (0, 0);
        };
        let Some(tasks) = self.tasks.get().and_then(Weak::upgrade) else {
            return (task.0, task.0);
        };
        let tgid = tasks
            .read_or_recover()
            .tasks
            .get(&task)
            .map_or(task.0, |t| t.process.pid.0);
        (task.0, tgid)
    }

    /// fanotify_alloc_event holds get_pid(pid), not just its current numeric ID.
    fn hold_pid(&self, id: u32) -> Option<crate::pid::PidRef> {
        if id == 0 {
            return None;
        }
        Some(self.tasks.get()?.upgrade()?.read_or_recover().pids.get(id))
    }

    fn vfs(&self) -> Option<Arc<Vfs>> {
        self.vfs.get().and_then(Weak::upgrade)
    }

    /// fsnotify_mnt: one committed attach/detach/move, after namespace_unlock.
    pub(crate) fn mount_event(&self, change: wasixv2_vfs::MountEvent) {
        if !self.watching() {
            return;
        }
        let mask = if change.attach {
            bit(FanotifyMask::FAN_MNT_ATTACH)
        } else {
            0
        } | if change.detach {
            bit(FanotifyMask::FAN_MNT_DETACH)
        } else {
            0
        };
        let mut current = None;
        for group in self
            .groups()
            .into_iter()
            .filter(|g| g.flags & REPORT_MNT != 0)
        {
            let matched = {
                let state = group.state.lock_or_recover();
                state.marks.iter().filter(|mark| matches!(mark.target, Target::Namespace(id, _) if id == change.namespace))
                    .fold(0, |bits, mark| bits | mask & mark.mask & !mark.ignore)
            };
            if matched != 0 {
                let (tid, tgid) = *current.get_or_insert_with(|| self.current());
                group.push(Event {
                    mask: matched,
                    tid: if group.flags & REPORT_TID != 0 {
                        tid
                    } else {
                        tgid
                    },
                    tgid,
                    pid: self.hold_pid(if group.flags & REPORT_TID != 0 {
                        tid
                    } else {
                        tgid
                    }),
                    info: Info::Mount(change.mount_id),
                    perm: None,
                });
            }
        }
    }

    pub(crate) fn mount_namespace_deleted(&self, namespace: u64) {
        for group in self.groups() {
            let removed = {
                let mut state = group.state.lock_or_recover();
                let before = state.marks.len();
                state.marks.retain(
                    |mark| !matches!(mark.target, Target::Namespace(id, _) if id == namespace),
                );
                before - state.marks.len()
            };
            group.uncount(removed);
        }
        self.recount();
    }

    /// `fsnotify`: hands `change` to every group with marks it matches.
    fn dispatch(&self, change: &Change<'_>) {
        if !self.marked.load(Ordering::Acquire) {
            return;
        }
        let (tid, tgid) = self.current();
        for group in self.groups() {
            if let Some(event) = group.event_for(self, change, tid, tgid) {
                group.push(event);
            }
        }
    }

    pub(crate) fn pre_content_watch(&self, path: &Path) -> bool {
        let object = FsObject {
            sb: path.dentry.sb().id,
            ino: path.dentry.node().ino(),
        };
        let parent = path.dentry.parent().map(|p| {
            (
                FsObject {
                    sb: p.sb().id,
                    ino: p.node().ino(),
                },
                path.dentry.name(),
            )
        });
        let change = Change {
            mask: PRE_ACCESS,
            object,
            path: Some(path),
            dir: parent.as_ref().map(|(dir, name)| (*dir, name.as_slice())),
            moved_to: None,
            self_only: false,
            range: None,
        };
        self.groups().iter().any(|group| {
            let (mask, _, _) = group.matching(&change);
            mask & PRE_ACCESS != 0
        })
    }

    /// The answer of the permission groups to `perm` on `path` (`fsnotify_perm`): asked of
    /// each in priority order, the first refusal ending it.
    pub(crate) fn permission(self: &Arc<Self>, path: &Path, perm: Perm) -> Option<PermAnswer> {
        if !self.marked.load(Ordering::Acquire) {
            return None;
        }
        if !matches!(
            path.dentry.file_type(),
            FileType::Regular | FileType::Directory
        ) {
            return None;
        }
        let mask = match perm {
            Perm::Open => OPEN_PERM,
            Perm::OpenExec => OPEN_PERM | OPEN_EXEC_PERM,
            Perm::Access => ACCESS_PERM,
            Perm::PreAccess { .. } => {
                if !path.dentry.sb().fs.allow_hsm() || path.dentry.is_dir() {
                    return None;
                }
                PRE_ACCESS
            }
        };
        let range = match perm {
            Perm::PreAccess { offset, count } => offset.map(|offset| {
                // fs/notify/fsnotify.c: fsnotify_pre_content reports a page-aligned range.
                let page = crate::mm::PAGE_SIZE;
                let start = offset / page * page;
                let end = offset
                    .saturating_add(count)
                    .div_ceil(page)
                    .saturating_mul(page);
                (start, end.saturating_sub(start))
            }),
            _ => None,
        };
        let is_dir = path.dentry.is_dir();
        let object = FsObject {
            sb: path.dentry.sb().id,
            ino: path.dentry.node().ino(),
        };
        let parent = path.dentry.parent().map(|parent| {
            let dir = FsObject {
                sb: parent.sb().id,
                ino: parent.node().ino(),
            };
            (dir, path.dentry.name())
        });
        let mut groups: Vec<Arc<Group>> = self
            .groups()
            .into_iter()
            .filter(|group| group.priority > 0)
            .collect();
        groups.sort_by_key(|group| std::cmp::Reverse(group.priority));
        let (tid, tgid) = self.current();
        let mut pending = Vec::new();
        for group in groups {
            let change = Change {
                mask: mask | if is_dir { ONDIR } else { 0 },
                object,
                path: Some(path),
                dir: parent.as_ref().map(|(dir, name)| (*dir, name.as_slice())),
                moved_to: None,
                self_only: false,
                range,
            };
            if let Some(event) = group.event_for(self, &change, tid, tgid) {
                pending.push((group, event));
            }
        }
        if pending.is_empty() {
            return None;
        }
        Some(Box::pin(async move {
            for (group, event) in pending {
                group.ask(event).await?;
            }
            Ok(())
        }))
    }
}

impl Group {
    fn fid_mode(&self) -> u32 {
        self.flags & FID_BITS
    }

    /// The marks of this group `change` reaches, with how each matched; `FAN_MODIFY` clears
    /// the ignore masks that do not survive it first (`send_to_group`).
    fn matching(&self, change: &Change<'_>) -> (u32, u32, u32) {
        let mut state = self.state.lock_or_recover();
        let dirent = change.mask & DIRENT_EVENTS != 0;
        let inode = match (dirent, change.dir) {
            (true, Some((dir, _))) => dir,
            _ => change.object,
        };
        let parent = match (dirent || change.self_only, change.dir) {
            (false, Some((dir, _))) => Some(dir),
            _ => None,
        };
        let types = |target: &Target| -> Option<Iter> {
            match *target {
                Target::Inode(object) if object == inode => Some(Iter::Inode),
                Target::Inode(object) if Some(object) == parent => Some(Iter::Parent),
                Target::Inode(object) if Some(object) == change.moved_to.map(|(d, _)| d) => {
                    Some(Iter::Inode2)
                }
                Target::Mount(id) if change.path.is_some_and(|p| p.mnt.id == id) => {
                    Some(Iter::Mount)
                }
                Target::Sb(sb) if sb == change.object.sb => Some(Iter::Sb),
                _ => None,
            }
        };
        let modify = change.mask & MODIFY != 0;
        let is_dir = change.mask & ONDIR != 0;
        let (mut mask, mut ignore, mut matched) = (0, 0, 0);
        for mark in state.marks.iter_mut() {
            let Some(iter) = types(&mark.target) else {
                continue;
            };
            if modify && !mark.surv_modify {
                mark.ignore = 0;
            }
            ignore |= mark.effective_ignore(is_dir, iter);
            if !applicable(mark.mask, is_dir, iter) {
                continue;
            }
            mask |= mark.mask;
            matched |= 1 << iter as u32;
        }
        (mask, ignore, matched)
    }

    /// `fanotify_group_event_mask` and `fanotify_alloc_event`: the event this group queues
    /// for `change`, if any.
    fn event_for(
        &self,
        registry: &Registry,
        change: &Change<'_>,
        tid: u32,
        tgid: u32,
    ) -> Option<Event> {
        let fid_mode = self.fid_mode();
        let ondir = change.mask & ONDIR != 0;
        let dir = change.dir.filter(|_| !change.self_only);
        if fid_mode == 0 {
            let path = change.path?;
            if !matches!(
                path.dentry.file_type(),
                FileType::Regular | FileType::Directory
            ) {
                return None;
            }
        } else if fid_mode & REPORT_FID == 0 && dir.is_none() && !ondir {
            return None;
        }
        let (marks, ignore, matched) = self.matching(change);
        let test = change.mask & marks & !ignore;
        let user = match fid_mode {
            0 => OUTGOING,
            _ => OUTGOING | EVENT_FLAGS,
        };
        if fid_mode != 0 && test & !EVENT_FLAGS == 0 {
            return None;
        }
        let mask = test & user;
        if mask & !EVENT_FLAGS == 0 {
            return None;
        }
        let info = match fid_mode {
            0 => Info::Path {
                path: change.path?.clone(),
                range: change.range,
            },
            _ => self.fid_info(registry, change, dir, mask, matched)?,
        };
        let perm = (mask & (PERM_EVENTS | PRE_ACCESS) != 0).then(|| {
            Arc::new(PermState {
                state: Mutex::new(PermProgress::Queued),
                answered: WaitQueue::new(),
            })
        });
        let tid = match self.flags & REPORT_TID {
            0 => tgid,
            _ => tid,
        };
        Some(Event {
            mask,
            tid,
            tgid,
            pid: registry.hold_pid(tid),
            info,
            perm,
        })
    }

    /// The identifiers a group with `FAN_REPORT_*FID*` reports of `change`
    /// (`fanotify_alloc_event`).
    fn fid_info(
        &self,
        registry: &Registry,
        change: &Change<'_>,
        dir: Option<(FsObject, &[u8])>,
        mask: u32,
        matched: u32,
    ) -> Option<Info> {
        let fid_mode = self.fid_mode();
        let vfs = registry.vfs()?;
        let dirent = mask & DIRENT_EVENTS != 0;
        let ondir = mask & ONDIR != 0;
        let dir_object = dir.map(|(dir, _)| dir);
        // `fanotify_fid_inode` and `fanotify_dfid_inode`.
        let mut id = match dirent && fid_mode & REPORT_TARGET_FID == 0 {
            true => dir_object,
            false => Some(change.object),
        };
        let dirid = match (dirent, ondir) {
            (true, _) => dir_object,
            (false, true) => Some(change.object),
            (false, false) => dir_object,
        };
        let mut name = dir.map(|(_, name)| name.to_vec());
        let mut child = None;
        let mut moved = None;
        let mut dir_fid = None;
        let mut name_event = false;
        if fid_mode & REPORT_DIR_FID != 0
            && let Some(dirid) = dirid
        {
            // `fanotify_report_child_fid`.
            let child_fid = match dirent {
                true => fid_mode & REPORT_TARGET_FID != 0,
                false => fid_mode & REPORT_FID != 0 && !ondir,
            };
            if child_fid && id != Some(dirid) {
                child = id;
            }
            id = Some(dirid);
            dir_fid = Some(dirid);
            if fid_mode & REPORT_NAME == 0 {
                name_event = child.is_some();
                name = None;
            } else if dirent || !ondir {
                name_event = true;
            }
            if mask & RENAME != 0 {
                let sb = matched & 1 << Iter::Sb as u32 != 0;
                let report_old = sb || matched & 1 << Iter::Inode as u32 != 0;
                let report_new = sb || matched & 1 << Iter::Inode2 as u32 != 0;
                if !report_old {
                    dir_fid = None;
                    name = None;
                }
                if report_new {
                    moved = change.moved_to;
                }
            }
        }
        let fsid = super::fsid(vfs.sb_dev(change.object.sb)?);
        let fh = |object: FsObject| -> Option<Fh> {
            let (fid, _) = vfs.object_fid(object)?;
            Some(Fh { fid })
        };
        if name_event && (name.is_some() || moved.is_some() || child.is_some()) {
            let dir = match dir_fid {
                Some(dir) => Some((fh(dir)?, name.unwrap_or_default())),
                None => None,
            };
            let dir2 = match moved {
                Some((to, to_name)) => Some((fh(to)?, to_name.to_vec())),
                None => None,
            };
            let object = match child {
                Some(child) => Some(fh(child)?),
                None => None,
            };
            return Some(Info::Fid {
                fsid,
                dir,
                dir2,
                object,
            });
        }
        Some(Info::Fid {
            fsid,
            dir: None,
            dir2: None,
            object: Some(fh(id?)?),
        })
    }

    /// `fanotify_merge` and `fsnotify_insert_event`: queues `event`, merged into an equal one
    /// queued lately, or the overflow event when the queue is full. Returns whether `event`
    /// was queued as itself.
    fn insert(&self, event: Event) -> bool {
        {
            let mut state = self.state.lock_or_recover();
            if state.shut {
                return false;
            }
            if event.perm.is_none() {
                let recent = state.queue.iter_mut().rev().take(MAX_MERGE);
                for old in recent.filter(|old| old.perm.is_none()) {
                    if old.merges_with(&event) {
                        old.mask |= event.mask;
                        return false;
                    }
                }
            }
            if state.queue.len() >= self.max_events {
                if state.overflowed {
                    return false;
                }
                state.overflowed = true;
                state.queue.push_back(Event {
                    mask: Q_OVERFLOW,
                    tid: 0,
                    tgid: 0,
                    pid: None,
                    info: Info::Overflow,
                    perm: None,
                });
                drop(state);
                self.wake();
                return false;
            }
            state.queue.push_back(event);
        }
        self.wake();
        true
    }

    fn push(&self, event: Event) {
        self.insert(event);
    }

    fn wake(&self) {
        self.wait.wake(PollMask::READABLE);
        self.fasync.kill(SiCode::POLL_IN);
    }

    /// Queues the permission event `event` and waits for its answer
    /// (`fanotify_get_response`): allowed unless the listener denies, with `EPERM` or the
    /// error it named. A queue too full for it lets the access through.
    async fn ask(&self, event: Event) -> Result<(), Errno> {
        let perm = event.perm.clone().expect("a permission event");
        if !self.insert(event) {
            return Ok(());
        }
        let answered = || match *perm.state.lock_or_recover() {
            PermProgress::Answered(response) => Some(response),
            _ => None,
        };
        // Only the caller's own wait ends early, and only for a fatal signal
        // (`LeaseWait::wait_killable`).
        let response = wait_event(&perm.answered, answered, &NoInterrupt)
            .await
            .unwrap_or(ALLOW);
        match response & (ALLOW | DENY) {
            ALLOW => Ok(()),
            _ => match (response >> ERRNO_SHIFT) & ERRNO_MASK {
                0 => Err(Errno::EPERM),
                errno => Err(Errno(errno as i32)),
            },
        }
    }

    /// `fanotify_release`'s answer to every pending permission event: allowed.
    fn shut_down(&self) {
        let pending: Vec<Arc<PermState>> = {
            let mut state = self.state.lock_or_recover();
            state.shut = true;
            let queued = state.queue.drain(..).filter_map(|event| event.perm);
            let mut pending: Vec<_> = queued.collect();
            pending.append(&mut state.access);
            state.marks.clear();
            pending
        };
        for perm in pending {
            perm.answer(ALLOW);
        }
        self.registry.recount();
    }

    /// `process_access_response`.
    fn respond(&self, fd: i32, response: u32, info: &[u8]) -> Result<usize, Errno> {
        if response & !RESPONSE_VALID != 0 {
            return Err(Errno::EINVAL);
        }
        let errno = (response >> ERRNO_SHIFT) & ERRNO_MASK;
        match response & (ALLOW | DENY) {
            ALLOW if errno == 0 => {}
            // A custom error is only for pre-content groups, which no filesystem allows.
            DENY if errno == 0 || self.priority == 2 => {
                let allowed = [Errno::EIO, Errno::EPERM, Errno::EBUSY, Errno::ETXTBSY];
                let more = [Errno::EAGAIN, Errno::ENOSPC, Errno::EDQUOT];
                let errno = Errno(errno as i32);
                if errno.0 != 0 && !allowed.contains(&errno) && !more.contains(&errno) {
                    return Err(Errno::EINVAL);
                }
            }
            _ => return Err(Errno::EINVAL),
        }
        // No group enables auditing.
        if response & AUDIT != 0 {
            return Err(Errno::EINVAL);
        }
        let consumed = match response & INFO {
            0 => 0,
            _ => {
                // `process_access_response_info`: one audit rule record.
                if info.len() != 16 || info[0] != 1 || info[1] != 0 {
                    return Err(Errno::EINVAL);
                }
                if u16::from_ne_bytes([info[2], info[3]]) != 16 {
                    return Err(Errno::EINVAL);
                }
                if fd == NOFD {
                    return Ok(16);
                }
                16
            }
        };
        if fd < 0 {
            return Err(Errno::EINVAL);
        }
        let perm = {
            let mut state = self.state.lock_or_recover();
            let at = state
                .access
                .iter()
                .position(|perm| *perm.state.lock_or_recover() == PermProgress::Reported(fd));
            at.map(|at| state.access.remove(at))
        };
        let perm = perm.ok_or(Errno::ENOENT)?;
        perm.answer(response);
        Ok(consumed)
    }

    fn queued_bytes(&self) -> usize {
        self.state.lock_or_recover().queue.len() * METADATA_LEN
    }
}

/// The open file of a group. Its last close shuts the group down.
pub(crate) struct FanotifyFile {
    group: Arc<Group>,
}

impl Drop for FanotifyFile {
    fn drop(&mut self) {
        self.group.shut_down();
        let mut counts = self.group.registry.counts.lock_or_recover();
        if let Some(count) = counts.get_mut(&self.group.owner) {
            count.0 = count.0.saturating_sub(1);
            let marks = self.group.state.lock_or_recover().marks.len();
            count.1 = count.1.saturating_sub(marks);
        }
    }
}

impl Pollable for FanotifyFile {
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.group.wait);
        match self.group.state.lock_or_recover().queue.is_empty() {
            true => PollMask::empty(),
            false => PollMask::READABLE,
        }
    }
}

impl FileObject for FanotifyFile {
    /// Reads go through [`Kernel::fanotify_read`], which opens and installs descriptors.
    fn read<'a>(&'a self, _: Io<'a>, _: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EINVAL.into()) })
    }

    /// `fanotify_write`: a `struct fanotify_response`, and an audit rule after it with
    /// `FAN_INFO`.
    fn write<'a>(&'a self, _: Io<'a>, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move {
            if data.len() < 8 {
                return Err(Errno::EINVAL.into());
            }
            let fd = i32::from_ne_bytes(data[..4].try_into().unwrap());
            let response = u32::from_ne_bytes(data[4..8].try_into().unwrap());
            let consumed = self.group.respond(fd, response, &data[8..])?;
            Ok(8 + consumed)
        })
    }

    fn fasync(&self, fasync: &Fasync<'_>) -> Option<Result<(), Errno>> {
        self.group.fasync.helper(fasync);
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
            let queued = self.group.queued_bytes() as i32;
            write_value(caller.memory(), GuestPtrMut::new(arg), &queued).await?;
            Ok(0)
        })
    }

    fn show_fdinfo(&self) -> String {
        self.group.show_fdinfo()
    }
}

impl Group {
    /// `fanotify_show_fdinfo`: the group's flags, then one line per mark.
    fn show_fdinfo(&self) -> String {
        let flags = self.flags & !UNPRIV;
        let mut out = format!(
            "fanotify flags:{flags:x} event-flags:{:x}\n",
            self.f_flags.0
        );
        let state = self.state.lock_or_recover();
        for mark in &state.marks {
            let mflags = match mark.ignore_flags || mark.surv_modify || mark.evictable {
                false => 0,
                true => {
                    u32::from(mark.surv_modify)
                        | u32::from(mark.evictable) << 1
                        | u32::from(mark.ignore_flags) << 2
                }
            };
            let tail = format!(
                "mflags:{mflags:x} mask:{:x} ignored_mask:{:x}\n",
                mark.mask, mark.ignore
            );
            let line = match mark.target {
                Target::Inode(object) => format!("fanotify ino:{:x} sdev:0 {tail}", object.ino),
                Target::Mount(id) => format!("fanotify mnt_id:{id:x} {tail}"),
                Target::Sb(_) => format!("fanotify sdev:0 {tail}"),
                Target::Namespace(_, ino) => format!("fanotify mnt_ns:{ino:x} {tail}"),
            };
            out.push_str(&line);
        }
        out
    }
}

impl Registry {
    /// `fsnotify_create`, `fsnotify_delete`, `fsnotify_move`, `fsnotify_change` and
    /// `fsnotify_file` in fanotify's terms.
    pub(crate) fn fsnotify(&self, event: &FsEvent<'_>, info: &EventInfo<'_>) {
        let flag = |is_dir: bool| if is_dir { ONDIR } else { 0 };
        let base = Change {
            mask: 0,
            object: FsObject { sb: 0, ino: 0 },
            path: None,
            dir: None,
            moved_to: None,
            self_only: false,
            range: None,
        };
        match *event {
            FsEvent::Create {
                dir,
                name,
                is_dir,
                object,
            } => self.dispatch(&Change {
                mask: CREATE | flag(is_dir),
                object,
                dir: Some((dir, name)),
                ..base
            }),
            FsEvent::Delete {
                dir,
                name,
                is_dir,
                object,
                gone,
            } => {
                if !gone {
                    self.dispatch(&Change {
                        mask: ATTRIB,
                        object,
                        self_only: true,
                        ..base
                    });
                }
                self.dispatch(&Change {
                    mask: DELETE | flag(is_dir),
                    object,
                    dir: Some((dir, name)),
                    ..base
                });
                if gone {
                    self.dispatch(&Change {
                        mask: DELETE_SELF | flag(is_dir),
                        object,
                        self_only: true,
                        ..base
                    });
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
                let isdir = flag(is_dir);
                self.dispatch(&Change {
                    mask: RENAME | isdir,
                    object,
                    dir: Some((from_dir, from_name)),
                    moved_to: Some((to_dir, to_name)),
                    ..base
                });
                self.dispatch(&Change {
                    mask: MOVED_FROM | isdir,
                    object,
                    dir: Some((from_dir, from_name)),
                    ..base
                });
                self.dispatch(&Change {
                    mask: MOVED_TO | isdir,
                    object,
                    dir: Some((to_dir, to_name)),
                    ..base
                });
                self.dispatch(&Change {
                    mask: MOVE_SELF | isdir,
                    object,
                    self_only: true,
                    ..base
                });
            }
            FsEvent::Modify { object }
            | FsEvent::Attrib { object }
            | FsEvent::Access { object }
            | FsEvent::Open { object, .. }
            | FsEvent::Close { object, .. } => {
                let mask = match *event {
                    FsEvent::Modify { .. } => MODIFY,
                    FsEvent::Attrib { .. } => ATTRIB,
                    FsEvent::Access { .. } => ACCESS,
                    FsEvent::Open { exec: true, .. } => OPEN | OPEN_EXEC,
                    FsEvent::Open { .. } => OPEN,
                    FsEvent::Close { wrote: true, .. } => CLOSE_WRITE,
                    _ => CLOSE_NOWRITE,
                };
                self.dispatch(&Change {
                    mask: mask | flag(info.is_dir),
                    object,
                    path: info.path,
                    dir: info.parent,
                    ..base
                });
            }
        }
    }
}

impl Kernel {
    /// `sys_fanotify_init`.
    pub(crate) fn sys_fanotify_init(
        &self,
        caller: &Caller<'_>,
        flags: u32,
        event_f_flags: u32,
    ) -> SysResult<i32> {
        let cred = self.process(caller.task()).cred();
        let admin = cred.capable_init(Cap::SysAdmin);
        if (flags & ADMIN_INIT != 0 || flags & (FID_BITS | REPORT_MNT) == 0) && !admin {
            return Err(Errno::EPERM.into());
        }
        if flags & !(ADMIN_INIT | USER_INIT) != 0 {
            return Err(Errno::EINVAL.into());
        }
        // fanotify_init: mount events have a notification-only group of their own.
        if flags & REPORT_MNT != 0 && flags & (CLASS_BITS | FID_BITS | REPORT_FD_ERROR) != 0 {
            return Err(Errno::EINVAL.into());
        }
        if event_f_flags & !EVENT_F_BITS != 0
            || event_f_flags & OpenFlags::O_ACCMODE.0 == OpenFlags::O_ACCMODE.0
        {
            return Err(Errno::EINVAL.into());
        }
        let fid_mode = flags & FID_BITS;
        let class = flags & CLASS_BITS;
        if fid_mode != 0 && class != 0 {
            return Err(Errno::EINVAL.into());
        }
        if fid_mode & REPORT_NAME != 0 && fid_mode & REPORT_DIR_FID == 0 {
            return Err(Errno::EINVAL.into());
        }
        if fid_mode & REPORT_TARGET_FID != 0
            && (fid_mode & REPORT_NAME == 0 || fid_mode & REPORT_FID == 0)
        {
            return Err(Errno::EINVAL.into());
        }
        let priority = match class {
            0 => 0,
            CLASS_CONTENT => 1,
            CLASS_PRE_CONTENT => 2,
            _ => return Err(Errno::EINVAL.into()),
        };
        let owner = cred.user.effective.0;
        {
            let mut counts = self.fanotify.counts.lock_or_recover();
            let count = counts.entry(owner).or_default();
            if count.0 >= MAX_GROUPS {
                return Err(Errno::EMFILE.into());
            }
            count.0 += 1;
        }
        let group = Arc::new(Group {
            flags: flags | if admin { 0 } else { UNPRIV },
            f_flags: OpenFlags(event_f_flags),
            priority,
            owner,
            user_ns: cred.user_ns.clone(),
            max_events: match flags & UNLIMITED_QUEUE {
                0 => MAX_EVENTS,
                _ => usize::MAX,
            },
            state: Mutex::default(),
            wait: WaitQueue::new(),
            fasync: FasyncList::default(),
            registry: self.fanotify.clone(),
        });
        self.fanotify
            .groups
            .lock_or_recover()
            .push(Arc::downgrade(&group));
        let object = FanotifyFile { group };
        let mut status = OpenFlags::O_RDWR;
        if flags & init(FanotifyInitFlags::FAN_NONBLOCK) != 0 {
            status |= OpenFlags::O_NONBLOCK;
        }
        let file = OpenFile::new(
            Backing::Stream(Arc::new(object), self.pseudo.anon("[fanotify]")),
            status,
        );
        let cloexec = flags & init(FanotifyInitFlags::FAN_CLOEXEC) != 0;
        Ok(self.install(caller, Arc::new(file), cloexec)?.0)
    }

    /// `do_fanotify_mark`.
    pub(crate) async fn sys_fanotify_mark(
        &self,
        caller: &Caller<'_>,
        fanotify_fd: Fd,
        flags: u32,
        mask: u64,
        dfd: Fd,
        pathname: Cstr,
    ) -> SysResult<i32> {
        if mask >> 32 != 0 || flags & !MARK_FLAGS != 0 {
            return Err(Errno::EINVAL.into());
        }
        let mut mask = mask as u32;
        let mark_type = flags & MARK_TYPE_BITS;
        if !matches!(mark_type, 0 | MARK_MOUNT | MARK_FILESYSTEM | MARK_MNTNS) {
            return Err(Errno::EINVAL.into());
        }
        let cmd = flags & (MARK_ADD | MARK_REMOVE | MARK_FLUSH);
        match cmd {
            MARK_ADD | MARK_REMOVE if mask == 0 => return Err(Errno::EINVAL.into()),
            MARK_ADD | MARK_REMOVE => {}
            MARK_FLUSH if flags & !(MARK_TYPE_BITS | MARK_FLUSH) != 0 => {
                return Err(Errno::EINVAL.into());
            }
            MARK_FLUSH => {}
            _ => return Err(Errno::EINVAL.into()),
        }
        if mask & !(EVENTS | EVENT_FLAGS | PERM_EVENTS | PRE_ACCESS) != 0 {
            return Err(Errno::EINVAL.into());
        }
        let ignore = flags & MARK_IGNORE_BITS;
        if ignore == MARK_IGNORE_BITS {
            return Err(Errno::EINVAL.into());
        }
        let mut umask = 0;
        if ignore == MARK_IGNORED_MASK {
            mask &= !EVENT_FLAGS;
            umask = EVENT_FLAGS;
        }
        let file = self.file(caller.task(), fanotify_fd)?;
        let group = file
            .downcast::<FanotifyFile>()
            .ok_or(Errno::EINVAL)?
            .group
            .clone();
        if group.flags & REPORT_MNT != 0 {
            if mask & !MNT_EVENTS != 0 || mark_type != MARK_MNTNS {
                return Err(Errno::EINVAL.into());
            }
        } else if mask & MNT_EVENTS != 0 || mark_type == MARK_MNTNS {
            return Err(Errno::EINVAL.into());
        }
        let cred = self.process(caller.task()).cred();
        if mark_type != 0 && !cred.ns_capable(&group.user_ns, Cap::SysAdmin) {
            return Err(Errno::EPERM.into());
        }
        if mask & (PERM_EVENTS | PRE_ACCESS) != 0 && group.priority == 0 {
            return Err(Errno::EINVAL.into());
        }
        if mask & PRE_ACCESS != 0 && group.priority == 1 {
            return Err(Errno::EINVAL.into());
        }
        if mask & FS_ERROR != 0 && mark_type != MARK_FILESYSTEM {
            return Err(Errno::EINVAL.into());
        }
        if flags & MARK_EVICTABLE != 0 && mark_type != 0 {
            return Err(Errno::EINVAL.into());
        }
        let fid_mode = group.fid_mode();
        if mask & !(FD_EVENTS | MNT_EVENTS | EVENT_FLAGS) != 0
            && (fid_mode == 0 || mark_type == MARK_MOUNT)
        {
            return Err(Errno::EINVAL.into());
        }
        if mask & RENAME != 0 && fid_mode & REPORT_NAME == 0 {
            return Err(Errno::EINVAL.into());
        }
        if mask & PRE_ACCESS != 0 && mask & ONDIR != 0 {
            return Err(Errno::EINVAL.into());
        }
        if cmd == MARK_FLUSH {
            group.flush(mark_type);
            return Ok(0);
        }
        let path = self.fanotify_path(caller, dfd, pathname, flags).await?;
        if cmd == MARK_ADD {
            fanotify_events_supported(&group, &path, mask, flags)?;
        }
        let env = self.env(caller);
        self.vfs.permission(&env.cx(), &path, MAY_READ).await?;
        if fid_mode != 0 {
            // `fanotify_test_fsid` and `fanotify_test_fid`: every filesystem here has a
            // non-zero fsid and encodes identifiers; mount and filesystem marks also need
            // handles that decode.
            let decodable = path.dentry.sb().fs.encode_fh(path.dentry.node().as_ref());
            if mark_type != 0 && decodable.is_none() {
                return Err(Errno::EOPNOTSUPP.into());
            }
        }
        let is_inode = mark_type == 0;
        let mount_ns = if mark_type == MARK_MNTNS {
            match crate::nsfs::namespace(&path) {
                Some(crate::ns::Namespace::Mnt(ns)) => Some(ns),
                _ => return Err(Errno::EINVAL.into()),
            }
        } else {
            None
        };
        let owner = mount_ns.as_ref().map_or_else(
            || self.sb_owner(path.dentry.sb()),
            |ns| ns.user_ns().clone(),
        );
        if !is_inode && !cred.ns_capable(&owner, Cap::SysAdmin) {
            return Err(Errno::EPERM.into());
        }
        let is_dir = path.dentry.is_dir();
        if cmd == MARK_ADD && ignore != 0 && flags & MARK_SURV_MODIFY == 0 {
            if ignore == MARK_IGNORE && (!is_inode || is_dir) {
                return Err(match is_inode {
                    true => Errno::EISDIR,
                    false => Errno::EINVAL,
                }
                .into());
            }
            if is_inode && path.dentry.inode().open_for_write() {
                return Ok(0);
            }
        }
        if !is_inode || !is_dir {
            mask &= !ON_CHILD;
            umask |= ON_CHILD;
            if fid_mode & REPORT_DIR_FID != 0 && cmd == MARK_ADD && ignore == 0 {
                mask |= ON_CHILD;
            }
        }
        let target = match mark_type {
            0 => Target::Inode(FsObject {
                sb: path.dentry.sb().id,
                ino: path.dentry.node().ino(),
            }),
            MARK_MOUNT => Target::Mount(path.mnt.id),
            MARK_MNTNS => {
                let ns = mount_ns.as_ref().unwrap();
                Target::Namespace(ns.tree.id(), crate::ns::Namespace::Mnt(ns.clone()).ino())
            }
            _ => Target::Sb(path.dentry.sb().id),
        };
        match cmd {
            MARK_ADD => group.add_mark(target, mask, flags, &path)?,
            _ => group.remove_mark(target, mask, flags, umask)?,
        }
        self.fanotify.recount();
        Ok(0)
    }

    /// `fanotify_find_path`: the object of a mark, by descriptor when there is no path.
    async fn fanotify_path(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        pathname: Cstr,
        flags: u32,
    ) -> SysResult<Path> {
        if pathname.is_null() {
            let path = self.fd_path(caller, dfd)?;
            if flags & MARK_ONLYDIR != 0 && !path.dentry.is_dir() {
                return Err(Errno::ENOTDIR.into());
            }
            return Ok(path);
        }
        let name = read_path(caller.memory(), pathname).await?;
        let env = self.env(caller);
        let follow = flags & MARK_DONT_FOLLOW == 0;
        let at = wasixv2_abi::AtFlags(0);
        let path = self
            .resolve_at(caller, &env, dfd, &name, at, follow)
            .await?;
        if flags & MARK_ONLYDIR != 0 && !path.dentry.is_dir() {
            return Err(Errno::ENOTDIR.into());
        }
        Ok(path)
    }

    /// `read` and `readv` of a group: `fanotify_read` for each buffer in turn
    /// (`do_loop_readv_writev`), until one is not filled.
    pub(crate) async fn fanotify_readv(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        object: &FanotifyFile,
        iovecs: &[wasixv2_abi::Iovec],
    ) -> SysResult<u32> {
        let mut total = 0;
        for iov in iovecs.iter().filter(|iov| iov.iov_len > 0) {
            let len = iov.iov_len as usize;
            let n = match self
                .fanotify_read(caller, file, &object.group, iov.iov_base, len)
                .await
            {
                Ok(n) => n,
                Err(err) if total == 0 => return Err(err),
                Err(_) => break,
            };
            total += n;
            if n != len {
                break;
            }
        }
        Ok(total as u32)
    }

    /// `fanotify_read`: whole events, each with a descriptor opened for it in the reader's
    /// table unless the group reports identifiers.
    async fn fanotify_read(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        group: &Arc<Group>,
        buf_addr: u32,
        count: usize,
    ) -> SysResult<usize> {
        let mut done = 0;
        let interrupt = self.interrupt(caller);
        loop {
            let event = {
                let take = || {
                    let mut state = group.state.lock_or_recover();
                    let Some(first) = state.queue.front() else {
                        return match (done > 0, file.flags().contains(OpenFlags::O_NONBLOCK)) {
                            (true, _) => Some(Ok(None)),
                            (false, true) => Some(Err(Errno::EAGAIN)),
                            (false, false) => None,
                        };
                    };
                    if first.len(group.flags) > count - done {
                        return Some(match done {
                            0 => Err(Errno::EINVAL),
                            _ => Ok(None),
                        });
                    }
                    let event = state.queue.pop_front().unwrap();
                    if event.mask == Q_OVERFLOW {
                        state.overflowed = false;
                    }
                    Some(Ok(Some(event)))
                };
                wait_event(&group.wait, take, &*interrupt)
                    .await
                    .map_err(|_| SysErr::Restart)??
            };
            let Some(event) = event else {
                return Ok(done);
            };
            match self
                .copy_event(caller, group, &event, buf_addr + done as u32)
                .await
            {
                Ok(len) => {
                    if let Some(perm) = &event.perm {
                        let fd = match *perm.state.lock_or_recover() {
                            PermProgress::Reported(fd) => fd,
                            _ => -1,
                        };
                        match fd >= 0 {
                            true => group.state.lock_or_recover().access.push(perm.clone()),
                            false => perm.answer(DENY),
                        }
                    }
                    done += len;
                }
                Err(err) => {
                    if let Some(perm) = &event.perm {
                        perm.answer(DENY);
                    }
                    return match done {
                        0 => Err(err),
                        _ => Ok(done),
                    };
                }
            }
        }
    }

    /// `copy_event_to_user`.
    async fn copy_event(
        &self,
        caller: &Caller<'_>,
        group: &Group,
        event: &Event,
        at: u32,
    ) -> SysResult<usize> {
        let len = event.len(group.flags);
        let reader_tgid = self.process(caller.task()).pid.0;
        let mut pid = event
            .pid
            .as_ref()
            .map_or(0, |pid| pid.nr(&self.process(caller.task()).pid_ns));
        if group.flags & UNPRIV != 0 && reader_tgid != event.tgid {
            pid = 0;
        }
        let mut fd = NOFD;
        let mut opened = None;
        if group.flags & UNPRIV == 0
            && let Info::Path { path, .. } = &event.info
        {
            match self.event_file(caller, group, path).await {
                Ok(file) => opened = Some(file),
                Err(errno) if group.flags & REPORT_FD_ERROR != 0 => fd = -errno.0,
                Err(errno) => return Err(errno.into()),
            }
        }
        let mut pidfd = None;
        if group.flags & REPORT_PIDFD != 0 {
            pidfd = Some(match pid {
                0 => FanotifyMisc::FAN_NOPIDFD.0,
                _ => match self.procs.find(event.tgid) {
                    Some(_) => -1,
                    None => FanotifyMisc::FAN_NOPIDFD.0,
                },
            });
        }
        let cloexec = group.f_flags.contains(OpenFlags::O_CLOEXEC);
        let reserved = match opened {
            Some(_) => Some(self.reserve_fds::<1>(caller, cloexec)?),
            None => None,
        };
        if let Some([slot]) = &reserved {
            fd = slot.fd().0;
        }
        let pidfd_file = match pidfd {
            Some(-1) => {
                let status = OpenFlags::O_RDWR;
                let process = match group.flags & REPORT_TID {
                    0 => event.tgid,
                    _ => event.tid,
                };
                let file = self.pidfd_file(process, status);
                Some(
                    self.reserve_fds::<1>(caller, true)
                        .map(|[slot]| (slot, file)),
                )
            }
            _ => None,
        };
        let pidfd_file = match pidfd_file {
            Some(Ok(pair)) => {
                pidfd = Some(pair.0.fd().0);
                Some(pair)
            }
            Some(Err(_)) => {
                pidfd = Some(FanotifyMisc::FAN_EPIDFD.0);
                None
            }
            None => None,
        };
        let mut out = Vec::with_capacity(len);
        out.extend_from_slice(&(len as u32).to_ne_bytes());
        out.push(FanotifyMisc::FANOTIFY_METADATA_VERSION.0 as u8);
        out.push(0);
        out.extend_from_slice(&(METADATA_LEN as u16).to_ne_bytes());
        out.extend_from_slice(&u64::from(event.mask & OUTGOING).to_ne_bytes());
        out.extend_from_slice(&fd.to_ne_bytes());
        out.extend_from_slice(&pid.to_ne_bytes());
        info_records(&mut out, event, group.flags);
        if let Some(pidfd) = pidfd {
            out.push(FanotifyMisc::FAN_EVENT_INFO_TYPE_PIDFD.0 as u8);
            out.push(0);
            out.extend_from_slice(&(PIDFD_INFO_LEN as u16).to_ne_bytes());
            out.extend_from_slice(&pidfd.to_ne_bytes());
        }
        debug_assert_eq!(out.len(), len);
        caller
            .memory()
            .write(at, &out)
            .await
            .map_err(|_| Errno::EFAULT)?;
        if let (Some([slot]), Some(file)) = (reserved, opened) {
            slot.install(file);
        }
        if let Some((slot, file)) = pidfd_file {
            slot.install(Arc::new(file));
        }
        if let Some(perm) = &event.perm {
            *perm.state.lock_or_recover() = PermProgress::Reported(fd);
        }
        Ok(len)
    }

    /// `create_fd`'s open: the event's object, as the reader, without events of its own.
    async fn event_file(
        &self,
        caller: &Caller<'_>,
        group: &Group,
        path: &Path,
    ) -> Result<Arc<OpenFile>, Errno> {
        let env = self.env(caller);
        let flags = group.f_flags;
        let file = self
            .vfs
            .dentry_open_nonotify(&env.cx(), path.clone(), flags)
            .await?;
        let backing = self
            .attach(caller, file, flags)
            .await
            .map_err(|err| match err {
                SysErr::Errno(errno) => errno,
                _ => Errno::EINTR,
            })?;
        Ok(Arc::new(OpenFile::new(backing, flags)))
    }

    /// `fsnotify_pre_content`, before the caller accesses or maps a file range.
    pub(crate) async fn pre_content_permission(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        offset: Option<u64>,
        count: u64,
    ) -> SysResult<()> {
        let Backing::Inode(inner) = &file.backing else {
            return Ok(());
        };
        if inner.nonotify() || !inner.path.dentry.sb().fs.allow_hsm() {
            return Ok(());
        }
        let env = self.env(caller);
        self.vfs
            .permission_event(&env.cx(), &inner.path, Perm::PreAccess { offset, count })
            .await
            .map_err(|errno| env.error(errno))
    }

    /// `fsnotify_file_area_perm` for a read of `file`: what its permission groups answer.
    pub(crate) async fn access_permission(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
    ) -> SysResult<()> {
        if !self.fanotify.marked.load(Ordering::Acquire) {
            return Ok(());
        }
        let Backing::Inode(inner) = &file.backing else {
            return Ok(());
        };
        if inner.nonotify() {
            return Ok(());
        }
        let env = self.env(caller);
        self.vfs
            .permission_event(&env.cx(), &inner.path, Perm::Access)
            .await
            .map_err(|errno| env.error(errno))
    }
}

/// `fanotify_events_supported`.
fn fanotify_events_supported(
    group: &Group,
    path: &Path,
    mask: u32,
    flags: u32,
) -> Result<(), Errno> {
    let mark_type = flags & MARK_TYPE_BITS;
    let is_dir = path.dentry.is_dir();
    let strict =
        group.flags & REPORT_TARGET_FID != 0 || mask & RENAME != 0 || flags & MARK_IGNORE != 0;
    if mask & PRE_ACCESS != 0 {
        if !path.dentry.sb().fs.allow_hsm() {
            return Err(Errno::EOPNOTSUPP);
        }
        if !is_dir && path.dentry.file_type() != FileType::Regular {
            return Err(Errno::EINVAL);
        }
    }
    // procfs (`FS_DISALLOW_NOTIFY_PERM`).
    if mask & PERM_EVENTS != 0 && path.dentry.sb().info.name == "proc" {
        return Err(Errno::EINVAL);
    }
    // Filesystems mounted nowhere (`SB_NOUSER`: pipes, sockets, anonymous inodes).
    if matches!(mark_type, MARK_MOUNT | MARK_FILESYSTEM) && path.dentry.is_pseudo() {
        return Err(Errno::EINVAL);
    }
    if strict && mark_type == 0 && !is_dir && mask & DIRONLY != 0 {
        return Err(Errno::ENOTDIR);
    }
    Ok(())
}

/// `copy_info_records_to_user`'s identifier records.
fn info_records(out: &mut Vec<u8>, event: &Event, flags: u32) {
    if let Info::Path {
        range: Some((offset, count)),
        ..
    } = event.info
    {
        out.extend_from_slice(&[FanotifyMisc::FAN_EVENT_INFO_TYPE_RANGE.0 as u8, 0]);
        out.extend_from_slice(&24u16.to_ne_bytes());
        out.extend_from_slice(&0u32.to_ne_bytes());
        out.extend_from_slice(&offset.to_ne_bytes());
        out.extend_from_slice(&count.to_ne_bytes());
        return;
    }

    if let Info::Mount(id) = event.info {
        out.extend_from_slice(&[FanotifyMisc::FAN_EVENT_INFO_TYPE_MNT.0 as u8, 0]);
        out.extend_from_slice(&16u16.to_ne_bytes());
        out.extend_from_slice(&0u32.to_ne_bytes());
        out.extend_from_slice(&id.to_ne_bytes());
        return;
    }

    let Info::Fid {
        fsid,
        dir,
        dir2,
        object,
    } = &event.info
    else {
        return;
    };
    let fid_mode = flags & FID_BITS;
    let mut info_type = 0;
    if let Some((fh, name)) = dir {
        info_type = match name.is_empty() {
            false => FanotifyMisc::FAN_EVENT_INFO_TYPE_DFID_NAME.0,
            true => FanotifyMisc::FAN_EVENT_INFO_TYPE_DFID.0,
        };
        if event.mask & RENAME != 0 {
            info_type = FanotifyMisc::FAN_EVENT_INFO_TYPE_OLD_DFID_NAME.0;
        }
        fid_record(out, *fsid, fh, info_type, name);
    }
    if let Some((fh, name)) = dir2 {
        let new = FanotifyMisc::FAN_EVENT_INFO_TYPE_NEW_DFID_NAME.0;
        fid_record(out, *fsid, fh, new, name);
    }
    if let Some(fh) = object {
        let fid = FanotifyMisc::FAN_EVENT_INFO_TYPE_FID.0;
        let (kind, name): (i32, &[u8]) = if fid_mode == REPORT_FID || info_type != 0 {
            (fid, b"")
        } else if fid_mode & REPORT_NAME != 0 && event.mask & ONDIR != 0 {
            (FanotifyMisc::FAN_EVENT_INFO_TYPE_DFID_NAME.0, b".")
        } else if event.mask & (DIRENT_EVENTS | ONDIR) != 0 {
            (FanotifyMisc::FAN_EVENT_INFO_TYPE_DFID.0, b"")
        } else {
            (fid, b"")
        };
        fid_record(out, *fsid, fh, kind, name);
    }
}

/// `copy_fid_info_to_user`: header, fsid, `struct file_handle`, the name with its NUL, padded
/// to four bytes.
fn fid_record(out: &mut Vec<u8>, fsid: [i32; 2], fh: &Fh, info_type: i32, name: &[u8]) {
    let body = fh.fid.data.len() + if name.is_empty() { 0 } else { name.len() + 1 };
    let len = (FID_INFO_HDR_LEN + body).next_multiple_of(4);
    let start = out.len();
    out.push(info_type as u8);
    out.push(0);
    out.extend_from_slice(&(len as u16).to_ne_bytes());
    out.extend_from_slice(&fsid[0].to_ne_bytes());
    out.extend_from_slice(&fsid[1].to_ne_bytes());
    out.extend_from_slice(&(fh.fid.data.len() as u32).to_ne_bytes());
    out.extend_from_slice(&fh.fid.kind.to_ne_bytes());
    out.extend_from_slice(&fh.fid.data);
    if !name.is_empty() {
        out.extend_from_slice(name);
        out.push(0);
    }
    out.resize(start + len, 0);
}

impl Group {
    /// `fanotify_add_mark`.
    fn add_mark(&self, target: Target, mask: u32, flags: u32, path: &Path) -> Result<(), Errno> {
        let ignore = flags & MARK_IGNORE_BITS;
        let mut state = self.state.lock_or_recover();
        let at = state.marks.iter().position(|mark| mark.target == target);
        let at = match at {
            Some(at) => at,
            None => {
                if self.flags & UNLIMITED_MARKS == 0 {
                    let mut counts = self.registry.counts.lock_or_recover();
                    let count = counts.entry(self.owner).or_default();
                    if count.1 >= MAX_MARKS {
                        return Err(Errno::ENOSPC);
                    }
                    count.1 += 1;
                }
                let evictable = flags & MARK_EVICTABLE != 0;
                let pin = match (target, evictable) {
                    (Target::Inode(_), false) => Some(path.dentry.clone()),
                    _ => None,
                };
                state.marks.push(Mark {
                    target,
                    mask: 0,
                    ignore: 0,
                    surv_modify: false,
                    ignore_flags: false,
                    evictable,
                    _pin: pin,
                });
                state.marks.len() - 1
            }
        };
        let mark = &mut state.marks[at];
        // `fanotify_may_update_existing_mark`.
        if flags & MARK_EVICTABLE != 0 && !mark.evictable {
            return Err(Errno::EEXIST);
        }
        if flags & MARK_IGNORED_MASK != 0 && mark.ignore_flags {
            return Err(Errno::EEXIST);
        }
        if flags & MARK_IGNORE != 0 && flags & MARK_SURV_MODIFY == 0 && mark.surv_modify {
            return Err(Errno::EEXIST);
        }
        // `fanotify_mark_add_to_mask` and `fanotify_mark_update_flags`.
        match ignore {
            0 => mark.mask |= mask,
            _ => mark.ignore |= mask,
        }
        if ignore == MARK_IGNORE {
            mark.ignore_flags = true;
        }
        if ignore != 0 && flags & MARK_SURV_MODIFY != 0 {
            mark.surv_modify = true;
        }
        if flags & MARK_EVICTABLE == 0 && mark.evictable {
            mark.evictable = false;
            if let Target::Inode(_) = target {
                mark._pin = Some(path.dentry.clone());
            }
        }
        Ok(())
    }

    /// `fanotify_remove_mark`.
    fn remove_mark(&self, target: Target, mask: u32, flags: u32, umask: u32) -> Result<(), Errno> {
        let mut state = self.state.lock_or_recover();
        let at = state
            .marks
            .iter()
            .position(|mark| mark.target == target)
            .ok_or(Errno::ENOENT)?;
        let mark = &mut state.marks[at];
        let mask = mask & !umask;
        match flags & MARK_IGNORE_BITS {
            0 => mark.mask &= !mask,
            _ => mark.ignore &= !mask,
        }
        if (mark.mask | mark.ignore) & !umask == 0 {
            state.marks.remove(at);
            drop(state);
            self.uncount(1);
        }
        Ok(())
    }

    /// `fsnotify_clear_marks_by_group` of one mark type.
    fn flush(&self, mark_type: u32) {
        let removed = {
            let mut state = self.state.lock_or_recover();
            let before = state.marks.len();
            state.marks.retain(|mark| {
                !matches!(
                    (mark.target, mark_type),
                    (Target::Inode(_), 0)
                        | (Target::Mount(_), MARK_MOUNT)
                        | (Target::Sb(_), MARK_FILESYSTEM)
                        | (Target::Namespace(..), MARK_MNTNS)
                )
            });
            before - state.marks.len()
        };
        self.uncount(removed);
        self.registry.recount();
    }

    fn uncount(&self, marks: usize) {
        if self.flags & UNLIMITED_MARKS != 0 {
            return;
        }
        let mut counts = self.registry.counts.lock_or_recover();
        if let Some(count) = counts.get_mut(&self.owner) {
            count.1 = count.1.saturating_sub(marks);
        }
    }
}
