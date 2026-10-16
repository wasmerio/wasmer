//! Change notification hooks (Linux's `fsnotify`).
//!
//! The VFS reports every change it makes through one [`FsWatcher`]; the kernel implements it to
//! feed inotify instances, matching watches by object identity (`sb`, `ino`). Changes made behind
//! the VFS's back (on the host side of a host directory) are not seen.

/// An object, identified the way watches are keyed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Object {
    pub sb: u64,
    pub ino: u64,
}

/// One change, in inotify's vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsEvent<'a> {
    /// `name` appeared in `dir` (`IN_CREATE`).
    Create {
        dir: Object,
        name: &'a [u8],
        is_dir: bool,
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
    /// Opened (`IN_OPEN`).
    Open { object: Object },
    /// Last reference of an open file went away (`IN_CLOSE_WRITE`/`IN_CLOSE_NOWRITE`).
    Close { object: Object, wrote: bool },
}

/// Receives change notifications. Called synchronously from the operation; must not block.
pub trait FsWatcher: Send + Sync {
    fn event(&self, event: &FsEvent<'_>);

    /// Whether anything is watched at all. While nothing is, the VFS skips the events of data
    /// access (`Access`, `Modify`, `Open`, `Close`, `Attrib`) without building them, as
    /// `fsnotify` returns early for objects without marks.
    fn is_watching(&self) -> bool {
        true
    }

    /// An object event (`Modify`, `Attrib`, `Access`, `Open`, `Close`) as the directory the
    /// object was reached through sees it, under its name there (`fsnotify_parent`). Follows
    /// `event` for the same change.
    fn child_event(&self, dir: Object, name: &[u8], event: &FsEvent<'_>) {
        let _ = (dir, name, event);
    }
}
