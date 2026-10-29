//! Path walking, after Linux's `fs/namei.c` (`link_path_walk`, `walk_component`, `pick_link`,
//! `follow_dotdot`, `lookup_last`).

use std::{collections::VecDeque, sync::Arc};

use wasixv2_abi::Errno;

use crate::{
    Vfs,
    cred::MAY_EXEC,
    driver::Link,
    mount::Path,
    ops::Ctx,
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
    /// `RESOLVE_NO_MAGICLINKS`: a magic link ([`Link::Jump`]) fails with `ELOOP`.
    pub no_magiclinks: bool,
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
    /// The walk ended on a magic link's object, which has no name to look up (`LAST_BIND`).
    Bind,
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
    cx: &'a Ctx<'a>,
    root: Path,
    /// Where the walk is: the directory holding the next component.
    pub(crate) path: Path,
    pub(crate) flags: LookupFlags,
    links: u32,
    stack: Vec<Segment>,
    /// The last step was a magic link's jump (`ND_JUMPED`).
    jumped: bool,
    /// The tree's shape count when the walk started, for scoped walks (`nd->r_seq`).
    tree_seq: u64,
}

impl<'a> Walk<'a> {
    /// Starts walking `path` from `start` (`path_init`).
    pub(crate) fn new(
        vfs: &'a Vfs,
        cx: &'a Ctx<'a>,
        start: &Path,
        path: &[u8],
        flags: LookupFlags,
    ) -> FsResult<Self> {
        let root = cx.root;
        if path.is_empty() && !flags.empty {
            return Err(Errno::ENOENT);
        }
        let root = if flags.beneath || flags.in_root {
            start.clone()
        } else {
            root.clone()
        };
        // `RESOLVE_IN_ROOT` walks an absolute path from `start` too, as Linux's `path_init`
        // does from `dfd`.
        let absolute = path.starts_with(b"/") && !flags.in_root;
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
            cx,
            root,
            path: current,
            flags,
            links: 0,
            stack: vec![Segment::parse(path)],
            jumped: false,
            tree_seq: vfs.tree_seq.read(),
        })
    }

    /// Walks every component but the last (`link_path_walk`), leaving `self.path` at the
    /// directory holding it.
    pub(crate) async fn walk_parent(&mut self) -> FsResult<Last> {
        loop {
            while self.stack.len() > 1 && self.stack.last().is_some_and(|s| s.comps.is_empty()) {
                self.stack.pop();
            }
            let end = if self.jumped { Last::Bind } else { Last::Root };
            let Some(top) = self.stack.last_mut() else {
                return Ok(end);
            };
            if top.comps.is_empty() {
                return Ok(end);
            }
            if !self.path.dentry.is_dir() {
                return Err(Errno::ENOTDIR);
            }
            self.vfs.permission(self.cx, &self.path, MAY_EXEC).await?;
            let top = self.stack.last_mut().expect("checked above");
            let comp = top.comps.pop_front().expect("checked above");
            self.jumped = false;
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
                Last::Dot | Last::Root | Last::Bind => {}
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
            Last::Root | Last::Dot | Last::Bind => Ok(Some(self.path.clone())),
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
        let _lock = self.path.dentry.inode().lock().await;
        self.child_locked(name).await
    }

    /// `child` with the current directory already locked by the caller.
    pub(crate) async fn child_locked(&self, name: &[u8]) -> FsResult<Path> {
        let mut path = lookup_one(&self.path, name).await?;
        if self.vfs.tree.follow_mounts(&mut path) && self.flags.no_xdev {
            return Err(Errno::EXDEV);
        }
        Ok(path)
    }

    /// Like `child_locked`, but `None` if the name does not exist.
    pub(crate) async fn child_opt_locked(&self, name: &[u8]) -> FsResult<Option<Path>> {
        match self.child_locked(name).await {
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
        if self.flags.no_symlinks || link.mnt.flags().nosymfollow {
            return Err(Errno::ELOOP);
        }
        // `may_follow_link`: the walk is in the link's directory.
        if self.vfs.protected(crate::Protection::Symlinks) != 0 {
            let dir = self.path.dentry.node().getattr().await?;
            let attr = link.dentry.node().getattr().await?;
            if !crate::cred::may_follow_link(self.cx.cred, &dir, &attr) {
                return Err(Errno::EACCES);
            }
        }
        let body = match link.dentry.node().get_link(self.cx).await? {
            Link::Body(body) => body,
            Link::Jump(target) => return self.jump(target),
        };
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

    /// `nd_jump_link`: the walk continues from a magic link's object. Not in scoped lookups,
    /// whose containment a jump would escape.
    fn jump(&mut self, target: Path) -> FsResult<()> {
        if self.flags.no_magiclinks {
            return Err(Errno::ELOOP);
        }
        if self.flags.no_xdev && !Arc::ptr_eq(&self.path.mnt, &target.mnt) {
            return Err(Errno::EXDEV);
        }
        if self.flags.beneath || self.flags.in_root {
            return Err(Errno::EXDEV);
        }
        while self.stack.len() > 1 && self.stack.last().is_some_and(|s| s.comps.is_empty()) {
            self.stack.pop();
        }
        self.path = target;
        self.jumped = true;
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

    /// `follow_dotdot`, and for a walk scoped to its start (`RESOLVE_BENEATH`,
    /// `RESOLVE_IN_ROOT`), `EAGAIN` if a rename or mount anywhere may have taken it out of the
    /// root meanwhile, so `..` could have left it: the caller retries (`handle_dots`).
    fn dotdot(&mut self) -> FsResult<()> {
        self.follow_dotdot()?;
        if (self.flags.beneath || self.flags.in_root) && self.vfs.tree_seq.read() != self.tree_seq {
            return Err(Errno::EAGAIN);
        }
        Ok(())
    }

    /// Never above the root, and from a mount's root to where it is mounted.
    fn follow_dotdot(&mut self) -> FsResult<()> {
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
        if self.vfs.tree.follow_mounts(&mut self.path) && self.flags.no_xdev {
            return Err(Errno::EXDEV);
        }
        Ok(())
    }
}

/// Asks the driver for `name` in `dir` and updates the name cache (`lookup_slow`), without
/// crossing mounts. The caller holds `dir`'s lock.
pub(crate) async fn lookup_one(dir: &Path, name: &[u8]) -> FsResult<Path> {
    if name.len() > dir.dentry.sb().info.name_max {
        return Err(Errno::ENAMETOOLONG);
    }
    if dir.dentry.inode().is_dead() {
        return Err(Errno::ENOENT);
    }
    // On a filesystem only this VFS changes, a cached name is still true (the dcache hit).
    if dir.dentry.sb().info.stable
        && let Some(cached) = dir.dentry.cached_child(name)
    {
        return Ok(Path {
            mnt: dir.mnt.clone(),
            dentry: cached,
        });
    }
    let node = match dir.dentry.node().lookup(name).await {
        Ok(node) => node,
        Err(Errno::ENOENT) => {
            dir.dentry.forget_name(name);
            return Err(Errno::ENOENT);
        }
        Err(errno) => return Err(errno),
    };
    Ok(Path {
        mnt: dir.mnt.clone(),
        dentry: dir.dentry.child(name, node),
    })
}

fn classify(comp: Vec<u8>) -> Last {
    match comp.as_slice() {
        b"." => Last::Dot,
        b".." => Last::DotDot,
        _ => Last::Normal(comp),
    }
}
