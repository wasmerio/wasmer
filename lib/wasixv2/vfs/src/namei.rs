//! Path walking, after Linux's `fs/namei.c` (`link_path_walk`, `walk_component`, `pick_link`,
//! `follow_dotdot`, `lookup_last`).

use std::{collections::VecDeque, sync::Arc};

use wasixv2_abi::Errno;

use crate::{
    Vfs,
    cred::{Cred, MAY_EXEC},
    mount::Path,
    types::{FileType, FsResult},
};

/// Linux's `MAXSYMLINKS`: symlinks one walk may follow in total.
pub const MAX_SYMLINKS: u32 = 40;

/// How a path is resolved (`LOOKUP_*` and the `openat2` `RESOLVE_*` flags).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LookupFlags {
    /// Follow a trailing symlink (`LOOKUP_FOLLOW`; the opposite of `AT_SYMLINK_NOFOLLOW`).
    pub follow: bool,
    /// The result must be a directory (`LOOKUP_DIRECTORY`, `O_DIRECTORY`).
    pub directory: bool,
    /// An empty path names the start directory (`AT_EMPTY_PATH`).
    pub empty: bool,
    /// `RESOLVE_NO_XDEV`: never cross a mount.
    pub no_xdev: bool,
    /// `RESOLVE_NO_SYMLINKS`: any symlink fails with `ELOOP`.
    pub no_symlinks: bool,
    /// `RESOLVE_BENEATH`: escaping the start directory fails with `EXDEV`.
    pub beneath: bool,
    /// `RESOLVE_IN_ROOT`: the start directory is the root for this walk.
    pub in_root: bool,
}

impl LookupFlags {
    pub fn follow() -> Self {
        Self {
            follow: true,
            ..Self::default()
        }
    }

    pub fn nofollow() -> Self {
        Self::default()
    }
}

/// The final component of a path, as the parent walk leaves it (`last_type`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Last {
    Normal(Vec<u8>),
    Dot,
    DotDot,
    /// No component at all: `/` or `//`.
    Root,
}

/// One string being walked: the path itself, or a symlink body.
struct Segment {
    comps: VecDeque<Vec<u8>>,
    trailing_slash: bool,
}

impl Segment {
    fn parse(path: &[u8]) -> Self {
        let comps: VecDeque<_> = path
            .split(|&b| b == b'/')
            .filter(|c| !c.is_empty())
            .map(<[u8]>::to_vec)
            .collect();
        Self {
            trailing_slash: path.ends_with(b"/") && !comps.is_empty(),
            comps,
        }
    }
}

/// A walk in progress (Linux's `nameidata`).
pub(crate) struct Walk<'a> {
    vfs: &'a Vfs,
    cred: &'a Cred,
    root: Path,
    /// Where the walk is: the directory holding the next component.
    pub(crate) path: Path,
    pub(crate) flags: LookupFlags,
    links: u32,
    stack: Vec<Segment>,
}

impl<'a> Walk<'a> {
    /// Starts walking `path` from `start` (`path_init`).
    pub(crate) fn new(
        vfs: &'a Vfs,
        cred: &'a Cred,
        root: &Path,
        start: &Path,
        path: &[u8],
        flags: LookupFlags,
    ) -> FsResult<Self> {
        if path.is_empty() && !flags.empty {
            return Err(Errno::ENOENT);
        }
        let root = if flags.beneath || flags.in_root {
            start.clone()
        } else {
            root.clone()
        };
        let absolute = path.starts_with(b"/");
        if absolute && flags.beneath {
            return Err(Errno::EXDEV);
        }
        if !absolute && !path.is_empty() && !start.dentry.is_dir() {
            return Err(Errno::ENOTDIR);
        }
        let current = if absolute {
            root.clone()
        } else {
            start.clone()
        };
        Ok(Self {
            vfs,
            cred,
            root,
            path: current,
            flags,
            links: 0,
            stack: vec![Segment::parse(path)],
        })
    }

    /// Walks every component but the last (`link_path_walk`), leaving `self.path` at the
    /// directory holding it.
    pub(crate) async fn walk_parent(&mut self) -> FsResult<Last> {
        loop {
            while self.stack.len() > 1 && self.stack.last().is_some_and(|s| s.comps.is_empty()) {
                self.stack.pop();
            }
            let Some(top) = self.stack.last_mut() else {
                return Ok(Last::Root);
            };
            if top.comps.is_empty() {
                return Ok(Last::Root);
            }
            if !self.path.dentry.is_dir() {
                return Err(Errno::ENOTDIR);
            }
            self.vfs.permission(self.cred, &self.path, MAY_EXEC).await?;
            let comp = top.comps.pop_front().expect("checked above");
            if self.stack.len() == 1 && self.stack[0].comps.is_empty() {
                let last = classify(comp);
                if self.stack[0].trailing_slash && matches!(last, Last::Normal(_)) {
                    // `lookup_last`: a trailing slash means "follow, and it must be a directory".
                    self.flags.follow = true;
                    self.flags.directory = true;
                }
                return Ok(last);
            }
            match classify(comp) {
                Last::Dot | Last::Root => {}
                Last::DotDot => self.dotdot()?,
                Last::Normal(name) => {
                    let child = self.child(&name).await?;
                    if child.dentry.file_type() == FileType::Symlink {
                        self.follow_link(&child).await?;
                    } else {
                        self.path = child;
                    }
                }
            }
        }
    }

    /// Resolves `last` after `walk_parent`. Returns `None` when it was a symlink to follow, in
    /// which case the caller walks again.
    pub(crate) async fn step_last(&mut self, last: &Last) -> FsResult<Option<Path>> {
        match last {
            Last::Root | Last::Dot => Ok(Some(self.path.clone())),
            Last::DotDot => {
                self.dotdot()?;
                Ok(Some(self.path.clone()))
            }
            Last::Normal(name) => {
                let child = self.child(name).await?;
                self.step_into(child).await
            }
        }
    }

    /// Moves onto `child`, the trailing component, following it if it is a symlink the walk
    /// follows.
    pub(crate) async fn step_into(&mut self, child: Path) -> FsResult<Option<Path>> {
        if child.dentry.file_type() == FileType::Symlink && self.flags.follow {
            self.follow_link(&child).await?;
            return Ok(None);
        }
        Ok(Some(child))
    }

    /// Resolves the whole path (`path_lookupat`).
    pub(crate) async fn resolve(&mut self) -> FsResult<Path> {
        loop {
            let last = self.walk_parent().await?;
            if let Some(path) = self.step_last(&last).await? {
                if self.flags.directory && !path.dentry.is_dir() {
                    return Err(Errno::ENOTDIR);
                }
                return Ok(path);
            }
        }
    }

    /// Looks `name` up in the current directory and crosses any mount on it.
    pub(crate) async fn child(&self, name: &[u8]) -> FsResult<Path> {
        let dir = &self.path;
        if name.len() > dir.dentry.sb().info.name_max {
            return Err(Errno::ENAMETOOLONG);
        }
        let node = dir.dentry.node().lookup(name).await?;
        let mut path = Path {
            mnt: dir.mnt.clone(),
            dentry: dir.dentry.child(name, node),
        };
        if self.vfs.ns.follow_mounts(&mut path) && self.flags.no_xdev {
            return Err(Errno::EXDEV);
        }
        Ok(path)
    }

    /// Like `child`, but `None` if the name does not exist.
    pub(crate) async fn child_opt(&self, name: &[u8]) -> FsResult<Option<Path>> {
        match self.child(name).await {
            Ok(path) => Ok(Some(path)),
            Err(Errno::ENOENT) => Ok(None),
            Err(errno) => Err(errno),
        }
    }

    /// `pick_link`: continues the walk with the body of `link`.
    async fn follow_link(&mut self, link: &Path) -> FsResult<()> {
        self.links += 1;
        if self.links > MAX_SYMLINKS {
            return Err(Errno::ELOOP);
        }
        if self.flags.no_symlinks || link.mnt.flags.nosymfollow {
            return Err(Errno::ELOOP);
        }
        let body = link.dentry.node().readlink().await?;
        if body.is_empty() {
            return Err(Errno::ENOENT);
        }
        if body.starts_with(b"/") {
            self.jump_root()?;
        }
        while self.stack.last().is_some_and(|s| s.comps.is_empty()) {
            self.stack.pop();
        }
        self.stack.push(Segment::parse(&body));
        Ok(())
    }

    /// `nd_jump_root`.
    fn jump_root(&mut self) -> FsResult<()> {
        if self.flags.beneath {
            return Err(Errno::EXDEV);
        }
        if self.flags.no_xdev && !Arc::ptr_eq(&self.path.mnt, &self.root.mnt) {
            return Err(Errno::EXDEV);
        }
        self.path = self.root.clone();
        Ok(())
    }

    /// `follow_dotdot`: never above the root, and from a mount's root to where it is mounted.
    fn dotdot(&mut self) -> FsResult<()> {
        loop {
            if self.path == self.root {
                return if self.flags.beneath {
                    Err(Errno::EXDEV)
                } else {
                    Ok(())
                };
            }
            if !self.path.is_mount_root() {
                break;
            }
            match self.path.mnt.parent() {
                Some(mountpoint) => {
                    if self.flags.no_xdev {
                        return Err(Errno::EXDEV);
                    }
                    self.path = mountpoint;
                }
                // The root of the namespace, outside the walk's root (after `chroot`).
                None => return Ok(()),
            }
        }
        if let Some(parent) = self.path.dentry.parent() {
            self.path.dentry = parent;
        }
        if self.vfs.ns.follow_mounts(&mut self.path) && self.flags.no_xdev {
            return Err(Errno::EXDEV);
        }
        Ok(())
    }
}

fn classify(comp: Vec<u8>) -> Last {
    match comp.as_slice() {
        b"." => Last::Dot,
        b".." => Last::DotDot,
        _ => Last::Normal(comp),
    }
}
