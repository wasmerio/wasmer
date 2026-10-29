//! Change notification hooks (Linux's `fsnotify`).
//!
//! The VFS reports every change it makes through one [`FsWatcher`]; the kernel implements it to
//! feed inotify instances, matching watches by object identity (`sb`, `ino`). Changes made behind
//! the VFS's back (on the host side of a host directory) are not seen.

use std::{future::Future, pin::Pin};

use crate::{mount::Path, types::FsResult};

/// An object, identified the way watches are keyed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Object {
    pub sb: u64,
    pub ino: u64,
}

/// One change, in inotify's vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsEvent<'a> {
    /// `name` appeared in `dir` (`IN_CREATE`), naming `object`.
    Create {
        dir: Object,
        name: &'a [u8],
        is_dir: bool,
        object: Object,
    },
    /// `name` was removed from `dir` (`IN_DELETE`); `object` lost a link (`IN_ATTRIB`, or
    /// `IN_DELETE_SELF` when it has none left).
    Delete {
        dir: Object,
        name: &'a [u8],
        is_dir: bool,
        object: Object,
        /// The object has no links left.
        gone: bool,
    },
    /// `IN_MOVED_FROM`/`IN_MOVED_TO` with a shared cookie, and `IN_MOVE_SELF`.
    Move {
        from_dir: Object,
        from_name: &'a [u8],
        to_dir: Object,
        to_name: &'a [u8],
        is_dir: bool,
        object: Object,
    },
    /// Data was written (`IN_MODIFY`).
    Modify { object: Object },
    /// Metadata changed (`IN_ATTRIB`).
    Attrib { object: Object },
    /// Data was read (`IN_ACCESS`).
    Access { object: Object },
    /// Opened (`IN_OPEN`); `exec` for a program `execve` loads (`FS_OPEN_EXEC`).
    Open { object: Object, exec: bool },
    /// Last reference of an open file went away (`IN_CLOSE_WRITE`/`IN_CLOSE_NOWRITE`).
    Close { object: Object, wrote: bool },
}

/// What [`FsWatcher::fsnotify`] learns of an event besides the event itself.
#[derive(Clone, Copy)]
pub struct EventInfo<'a> {
    /// The directory the object is in and its name there, or had when it was removed; `None`
    /// for an event about a directory's entries, and for an object with no parent.
    pub parent: Option<(Object, &'a [u8])>,
    /// The path a data-access event's object was used through.
    pub path: Option<&'a Path>,
    /// The object of an object event is a directory.
    pub is_dir: bool,
}

/// A permission event (`fsnotify_open_perm`, `fsnotify_file_area_perm`): asked before the
/// access happens, which a listener may refuse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Perm {
    /// `FS_OPEN_PERM`.
    Open,
    /// `FS_OPEN_EXEC_PERM`, with `FS_OPEN_PERM`: opened as a program for `execve`.
    OpenExec,
    /// `FS_ACCESS_PERM`: a read.
    Access,
    /// `FS_PRE_ACCESS` before accessing a file range. No offset means an unknown range.
    PreAccess { offset: Option<u64>, count: u64 },
}

/// A committed mount-tree change (`fs/namespace.c: mnt_notify`). The mount id is
/// the unique id reported by statmount, not the reusable legacy mount id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MountEvent {
    pub namespace: u64,
    pub mount_id: u64,
    pub attach: bool,
    pub detach: bool,
}

/// The answer to a permission event, once its listeners gave it: `Ok` to go on, or the error
/// the access fails with.
pub type PermAnswer = Pin<Box<dyn Future<Output = FsResult<()>>>>;

/// Receives change notifications. Called synchronously from the operation; must not block.
pub trait FsWatcher: Send + Sync {
    fn event(&self, event: &FsEvent<'_>);

    /// Whether opening this file sets FMODE_FSNOTIFY_HSM (fsnotify_open).
    fn pre_content_watch(&self, _path: &Path) -> bool {
        false
    }

    /// Delivered after releasing the mount-tree lock, as namespace_unlock does.
    fn mount_event(&self, _event: MountEvent) {}

    /// The namespace has gone; remove marks on it (`fsnotify_mntns_delete`).
    fn mount_namespace_deleted(&self, _namespace: u64) {}

    /// Whether any watch exists. While none does, the VFS builds no events at all, so this must
    /// be cheap (an atomic load) and become true before the first watch can see an event.
    fn watching(&self) -> bool {
        true
    }

    /// An object event (`Modify`, `Attrib`, `Access`, `Open`, `Close`) on an object whose
    /// name was removed while it stayed open (`d_unlinked`), for its own watches; inotify's
    /// `IN_EXCL_UNLINK` watches skip it. Takes the place of `event` for such a change.
    fn unlinked_event(&self, event: &FsEvent<'_>) {
        self.event(event);
    }

    /// An object event (`Modify`, `Attrib`, `Access`, `Open`, `Close`) as the directory the
    /// object was reached through sees it, under its name there (`fsnotify_parent`), or the
    /// directory and name it had, when `unlinked`. Follows `event` for the same change.
    fn child_event(&self, dir: Object, name: &[u8], event: &FsEvent<'_>, unlinked: bool) {
        let _ = (dir, name, event, unlinked);
    }

    /// Every change once more, with what fanotify reports of it (`fsnotify`): for an event
    /// about an object, the directory and name it was reached through (`fsnotify_parent`) and,
    /// for data access, the path it was used through (`FSNOTIFY_EVENT_PATH`). Follows the
    /// calls above for the same change.
    fn fsnotify(&self, event: &FsEvent<'_>, info: &EventInfo<'_>) {
        let _ = (event, info);
    }

    /// Asks the watches that want `perm` on the object `path` names whether the access may
    /// happen (`fsnotify_perm`); `None` when none does. The caller waits for the answer
    /// before the access, killable only.
    fn permission(&self, path: &Path, perm: Perm) -> Option<PermAnswer> {
        let _ = (path, perm);
        None
    }
}
