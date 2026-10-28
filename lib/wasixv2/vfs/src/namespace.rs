//! Changing mount trees: `mount(2)`'s operations, `umount2`, `pivot_root`, `mount_setattr`
//! and copying a namespace, after Linux's `fs/namespace.c`.
//!
//! The kernel resolves the paths in the caller's context and checks the caller may mount
//! (`may_mount`, `CAP_SYS_ADMIN`) before it calls these; each takes the caller's namespace,
//! which the mounts it changes must belong to (`check_mnt`).

use std::sync::{Arc, atomic::Ordering};

use wasixv2_abi::Errno;

use crate::{
    Vfs,
    dentry::Dentry,
    driver::Filesystem,
    mount::{MntNs, Mount, MountFlags, Path, Superblock, TreeState, is_path_reachable},
    pnode::{CopyFlags, Propagation},
    types::FsResult,
};

/// What `mount(2)` does to its target (`path_mount`'s dispatch).
pub enum MountOp {
    /// Mounts a new filesystem instance, made from `source` (`do_new_mount`).
    New {
        fs: Arc<dyn Filesystem>,
        source: Vec<u8>,
        flags: MountFlags,
    },
    /// Mounts the tree at `source` again, with what is mounted below it if `recursive`
    /// (`do_loopback`, `MS_BIND` and `MS_REC`).
    Bind { source: Path, recursive: bool },
    /// Moves the mount whose root `source` is (`do_move_mount_old`, `MS_MOVE`).
    Move { source: Path },
    /// Changes the filesystem's read-only state and the mount's flags (`do_remount`).
    Remount { read_only: bool, flags: MountFlags },
    /// Changes only the mount's flags (`do_reconfigure_mnt`, `MS_REMOUNT | MS_BIND`).
    Reconfigure { flags: MountFlags },
    /// Changes the propagation of the mount, and of all below it if `recursive`
    /// (`do_change_type`).
    Propagation { kind: Propagation, recursive: bool },
}

/// `umount2` flags.
#[derive(Clone, Copy, Debug, Default)]
pub struct UmountFlags {
    /// `MNT_FORCE`: no filesystem here has operations to abort, so it changes nothing.
    pub force: bool,
    /// `MNT_DETACH`: take the tree out now, whoever still uses it.
    pub detach: bool,
    /// `MNT_EXPIRE`: mark an unused mount, and unmount it if still marked next time.
    pub expire: bool,
}

/// `mount_setattr`'s change to a mount's flags and propagation (`struct mount_kattr`). The
/// flags set in `clear` are cleared, then those in `set` set (`recalc_flags`).
#[derive(Clone, Copy, Debug, Default)]
pub struct MountAttr {
    pub set: MountFlags,
    pub clear: MountFlags,
    pub propagation: Option<Propagation>,
    /// `AT_RECURSIVE`: every mount below too.
    pub recursive: bool,
}

/// A mount as `/proc/<pid>/mountinfo` shows it (`show_mountinfo`).
pub struct MountInfo {
    pub mount: Arc<Mount>,
    /// The mount it is attached on; its own id for the namespace root.
    pub parent: u64,
    /// `shared:N`: its peer group.
    pub shared: Option<u32>,
    /// `master:N`: the group it receives from.
    pub master: Option<u32>,
    /// `propagate_from:N`: the closest group it gets events from that has a member under the
    /// reader's root, when that is not its master.
    pub propagate_from: Option<u32>,
    pub unbindable: bool,
}

/// `cant_mount`: a removed name, or an object its filesystem keeps mounts off.
fn cant_mount(dentry: &Dentry) -> bool {
    dentry.is_unlinked() || dentry.node().unmountable()
}

fn check_mnt(state: &TreeState, ns: &MntNs, mount: &Mount) -> bool {
    state.ns_of(mount.id) == Some(ns.id())
}

/// `set_mount_attributes`: the flags a user can set, all replaced.
fn set_mount_attributes(mount: &Mount, flags: MountFlags) {
    mount.set_flags(flags);
}

/// The mount `mount` is attached on, or itself for a root (`mnt_parent`).
fn parent_mount(mount: &Arc<Mount>) -> Arc<Mount> {
    mount.parent().map_or_else(|| mount.clone(), |at| at.mnt)
}

/// `mount_is_ancestor`: whether `mount` is `ancestor` or below it.
fn mount_is_ancestor(ancestor: &Arc<Mount>, mount: &Arc<Mount>) -> bool {
    let mut current = mount.clone();
    loop {
        if Arc::ptr_eq(&current, ancestor) {
            return true;
        }
        match current.parent() {
            Some(at) => current = at.mnt,
            None => return false,
        }
    }
}

impl Vfs {
    /// Mounts on `target` as `op` says (`path_mount`).
    pub fn do_mount(&self, ns: &MntNs, target: &Path, op: MountOp) -> FsResult<()> {
        match op {
            MountOp::New { fs, source, flags } => self.new_mount(ns, target, fs, source, flags),
            MountOp::Bind { source, recursive } => self.loopback(ns, target, &source, recursive),
            MountOp::Move { source } => self.move_mount(ns, &source, target),
            MountOp::Remount { read_only, flags } => {
                let state = self.tree.lock();
                if !check_mnt(&state, ns, &target.mnt) || !target.is_mount_root() {
                    return Err(Errno::EINVAL);
                }
                target.mnt.sb().set_read_only(read_only);
                set_mount_attributes(&target.mnt, flags);
                Ok(())
            }
            MountOp::Reconfigure { flags } => {
                let state = self.tree.lock();
                if !check_mnt(&state, ns, &target.mnt) || !target.is_mount_root() {
                    return Err(Errno::EINVAL);
                }
                set_mount_attributes(&target.mnt, flags);
                Ok(())
            }
            MountOp::Propagation { kind, recursive } => {
                self.change_type(target, Some(kind), recursive, None)
            }
        }
    }

    /// `do_new_mount_fc` and `do_add_mount`.
    fn new_mount(
        &self,
        ns: &MntNs,
        target: &Path,
        fs: Arc<dyn Filesystem>,
        source: Vec<u8>,
        flags: MountFlags,
    ) -> FsResult<()> {
        let sb = self.new_superblock(fs);
        let root = Dentry::root(&sb, sb.fs.root());
        let mount = Arc::new(Mount::new(self.tree.next_id(), root, flags, source));
        let mut state = self.tree.lock();
        let at = self.lock_mount(&state, target)?;
        if !check_mnt(&state, ns, &at.mnt) {
            return Err(Errno::EINVAL);
        }
        state.add_link(&mount, 0);
        let result = self.graft_tree(&mut state, &mount, &at);
        if result.is_err() {
            state.umount_tree(&mount, false);
        }
        result
    }

    /// `do_lock_mount`: where a mount on `target` goes, on top of anything mounted there,
    /// unless that place takes no mounts.
    fn lock_mount(&self, state: &TreeState, target: &Path) -> FsResult<Path> {
        let at = state.where_to_mount(target);
        if cant_mount(&at.dentry) {
            return Err(Errno::ENOENT);
        }
        Ok(at)
    }

    /// `graft_tree`: attaches the new tree at `mount` on `at`.
    fn graft_tree(&self, state: &mut TreeState, mount: &Arc<Mount>, at: &Path) -> FsResult<()> {
        if at.dentry.is_dir() != mount.root.is_dir() {
            return Err(Errno::ENOTDIR);
        }
        self.attach_recursive(state, mount, at, false)
    }

    /// `attach_recursive_mnt`: attaches the tree at `source` on `at`, with copies for every
    /// mount that receives from `at`'s mount if that is shared. A tree that is `moving` is
    /// taken off where it was.
    fn attach_recursive(
        &self,
        state: &mut TreeState,
        source: &Arc<Mount>,
        at: &Path,
        moving: bool,
    ) -> FsResult<()> {
        let dest = &at.mnt;
        let ns = state.ns_of(dest.id).unwrap_or(0);
        let tree = state.subtree(source);
        let shared = state.is_shared(dest);
        let mut copies = Vec::new();
        if shared {
            let invented = state.invent_group_ids(&tree);
            match state.propagate_mnt(&self.tree.next_id, dest, &at.dentry, source) {
                Ok(made) => copies = made,
                Err(errno) => {
                    state.cleanup_group_ids(&invented);
                    return Err(errno);
                }
            }
        }
        self.tree.seq.bump();
        if moving {
            state.detach(source);
        }
        state.attach(source, at);
        state.set_ns(&tree, ns);
        for (place, copy) in copies {
            let covering = state.mounted_on(&place);
            if let Some(covering) = &covering {
                state.detach(covering);
            }
            state.attach(&copy, &place);
            let copied = state.subtree(&copy);
            let place_ns = state.ns_of(place.mnt.id).unwrap_or(0);
            state.set_ns(&copied, place_ns);
            // What was there goes on top of the copy (`mnt_change_mountpoint`).
            if let Some(covering) = covering {
                let top = state.topmost(&copy);
                state.attach(&covering, &top.root_path());
            }
        }
        Ok(())
    }

    /// `do_loopback`.
    fn loopback(&self, ns: &MntNs, target: &Path, source: &Path, recursive: bool) -> FsResult<()> {
        let mut state = self.tree.lock();
        let at = self.lock_mount(&state, target)?;
        if !check_mnt(&state, ns, &at.mnt) {
            return Err(Errno::EINVAL);
        }
        // `__do_loopback`: the source must be in the caller's namespace, bindable, and not an
        // object without a name.
        let old = &source.mnt;
        if state.is_unbindable(old) || !check_mnt(&state, ns, old) || source.dentry.is_pseudo() {
            return Err(Errno::EINVAL);
        }
        let mount = match recursive {
            true => {
                let flags = CopyFlags::default();
                state
                    .copy_tree(&self.tree.next_id, old, &source.dentry, flags)?
                    .0
            }
            false => state.clone_mnt(
                &self.tree.next_id,
                old,
                source.dentry.clone(),
                CopyFlags::default(),
            ),
        };
        let result = self.graft_tree(&mut state, &mount, &at);
        if result.is_err() {
            state.umount_tree(&mount, false);
        }
        result
    }

    /// `do_move_mount` for a mount of the caller's namespace.
    fn move_mount(&self, ns: &MntNs, source: &Path, target: &Path) -> FsResult<()> {
        if !source.is_mount_root() {
            return Err(Errno::EINVAL);
        }
        if target.dentry.is_dir() != source.dentry.is_dir() {
            return Err(Errno::EINVAL);
        }
        let mut state = self.tree.lock();
        let at = self.lock_mount(&state, target)?;
        let old = &source.mnt;
        // Only a mount of the caller's namespace that has a parent, which does not propagate,
        // moves, and only within the namespace. (Detached trees of `open_tree` would be the
        // other case.)
        if !check_mnt(&state, ns, old) {
            return Err(Errno::EINVAL);
        }
        let Some(from) = old.parent() else {
            return Err(Errno::EINVAL);
        };
        if state.is_shared(&from.mnt) || !check_mnt(&state, ns, &at.mnt) {
            return Err(Errno::EINVAL);
        }
        let tree = state.subtree(old);
        if state.is_shared(&at.mnt) && tree.iter().any(|m| state.is_unbindable(m)) {
            return Err(Errno::EINVAL);
        }
        if mount_is_ancestor(old, &at.mnt) {
            return Err(Errno::ELOOP);
        }
        self.attach_recursive(&mut state, old, &at, true)
    }

    /// `do_change_type`, and the propagation part of `mount_setattr`: `kind` for the mount
    /// whose root `target` is and, if `recursive`, every mount below it. `flags`, if given,
    /// replaces the flags of each the same way.
    fn change_type(
        &self,
        target: &Path,
        kind: Option<Propagation>,
        recursive: bool,
        attr: Option<&MountAttr>,
    ) -> FsResult<()> {
        if !target.is_mount_root() {
            return Err(Errno::EINVAL);
        }
        let mut state = self.tree.lock();
        // `may_change_propagation`: the mount must be in some namespace.
        if state.ns_of(target.mnt.id).is_none() {
            return Err(Errno::EINVAL);
        }
        let mounts = match recursive {
            true => state.subtree(&target.mnt),
            false => vec![target.mnt.clone()],
        };
        if kind == Some(Propagation::Shared) {
            state.invent_group_ids(&mounts);
        }
        for mount in &mounts {
            if let Some(attr) = attr {
                mount.set_flags(recalc_flags(mount.flags(), attr));
            }
            if let Some(kind) = kind {
                state.change_propagation(mount.id, kind);
            }
        }
        self.tree.seq.bump();
        Ok(())
    }

    /// `do_mount_setattr`: changes the flags and propagation of the mount whose root `path` is
    /// (and of every mount below with `attr.recursive`).
    pub fn mount_setattr(&self, ns: &MntNs, path: &Path, attr: &MountAttr) -> FsResult<()> {
        if !path.is_mount_root() {
            return Err(Errno::EINVAL);
        }
        if !check_mnt(&self.tree.lock(), ns, &path.mnt) {
            return Err(Errno::EINVAL);
        }
        self.change_type(path, attr.propagation, attr.recursive, Some(attr))
    }

    /// `path_umount`: unmounts the mount whose root `path` is. The caller holds one reference to
    /// it, through `path`; any other besides the tree's makes it busy unless detaching. A
    /// caller unmounting its own root without `MNT_DETACH` makes the filesystem read-only
    /// instead (`do_umount_root`).
    pub fn do_umount(
        &self,
        ns: &MntNs,
        caller_root: &Path,
        path: &Path,
        flags: UmountFlags,
    ) -> FsResult<()> {
        let mount = &path.mnt;
        // `can_umount`.
        if !path.is_mount_root() || !check_mnt(&self.tree.lock(), ns, mount) {
            return Err(Errno::EINVAL);
        }
        let is_root = Arc::ptr_eq(mount, &caller_root.mnt);
        if flags.expire {
            if is_root || flags.force || flags.detach {
                return Err(Errno::EINVAL);
            }
            let state = self.tree.lock();
            if state.has_children(mount) || Arc::strong_count(mount) != 2 {
                return Err(Errno::EBUSY);
            }
            if !mount.expiry_mark.swap(true, Ordering::Relaxed) {
                return Err(Errno::EAGAIN);
            }
        }
        if is_root && !flags.detach {
            mount.sb().set_read_only(true);
            return Ok(());
        }
        let _gone = {
            let mut state = self.tree.lock();
            if state.ns_of(mount.id) != Some(ns.id()) || mount.parent().is_none() {
                return Err(Errno::EINVAL);
            }
            // The tree's reference and the caller's.
            if !flags.detach && state.propagate_mount_busy(mount, 2) {
                return Err(Errno::EBUSY);
            }
            self.tree.seq.bump();
            state.umount_tree(mount, true)
        };
        Ok(())
    }

    /// `path_pivot_root`: in the caller's namespace, the mount at `new_root` takes the place of
    /// the caller's root mount (`root`), which moves to `put_old`. The kernel then moves every
    /// process whose root or working directory was `root` to `new_root` (`chroot_fs_refs`).
    pub fn pivot_root(
        &self,
        ns: &MntNs,
        root: &Path,
        new_root: &Path,
        put_old: &Path,
    ) -> FsResult<()> {
        let mut state = self.tree.lock();
        let old_at = self.lock_mount(&state, put_old)?;
        let old_mnt = &old_at.mnt;
        let new_mnt = &new_root.mnt;
        let root_mnt = &root.mnt;
        if !check_mnt(&state, ns, root_mnt) || !check_mnt(&state, ns, new_mnt) {
            return Err(Errno::EINVAL);
        }
        if state.is_shared(old_mnt)
            || state.is_shared(&parent_mount(new_mnt))
            || state.is_shared(&parent_mount(root_mnt))
        {
            return Err(Errno::EINVAL);
        }
        if new_root.dentry.is_unlinked() {
            return Err(Errno::ENOENT);
        }
        if Arc::ptr_eq(new_mnt, root_mnt) || Arc::ptr_eq(old_mnt, root_mnt) {
            return Err(Errno::EBUSY);
        }
        if !root.is_mount_root() || root_mnt.parent().is_none() {
            return Err(Errno::EINVAL);
        }
        if !new_root.is_mount_root() || new_mnt.parent().is_none() {
            return Err(Errno::EINVAL);
        }
        if !is_path_reachable(old_mnt, &old_at.dentry, new_root) {
            return Err(Errno::EINVAL);
        }
        if !is_path_reachable(new_mnt, &new_root.dentry, root) {
            return Err(Errno::EINVAL);
        }
        self.tree.seq.bump();
        let root_mp = state.detach(root_mnt).expect("checked above");
        state.detach(new_mnt);
        state.attach(new_mnt, &root_mp);
        state.attach(root_mnt, &old_at);
        Ok(())
    }

    /// `copy_mnt_ns`: see [`MntNs::copy`].
    pub fn copy_mnt_ns(&self, from: &MntNs, paths: &mut [&mut Path]) -> Arc<MntNs> {
        from.copy(paths)
    }

    /// The mounts of `ns`, oldest first, with their propagation as seen from `root`.
    pub fn mount_info(&self, ns: &MntNs, root: &Path) -> Vec<MountInfo> {
        let state = self.tree.lock();
        state
            .mounts_in(ns.id())
            .into_iter()
            .map(|mount| {
                let (group, master, unbindable) = state.propagation_of(&mount);
                let dominating = match master {
                    0 => 0,
                    _ => state.dominating_group(&mount, root),
                };
                MountInfo {
                    parent: mount.parent().map_or(mount.id, |at| at.mnt.id),
                    shared: (group != 0).then_some(group),
                    master: (master != 0).then_some(master),
                    propagate_from: (dominating != 0 && dominating != master).then_some(dominating),
                    unbindable,
                    mount,
                }
            })
            .collect()
    }

    /// Every filesystem mounted in some namespace, once each (`iterate_supers`).
    pub fn superblocks(&self) -> Vec<Arc<Superblock>> {
        let state = self.tree.lock();
        let mut found: Vec<Arc<Superblock>> = Vec::new();
        for link in state.links.values() {
            if let Some(mount) = link.mount.upgrade()
                && link.ns != 0
                && !found.iter().any(|sb| Arc::ptr_eq(sb, mount.sb()))
            {
                found.push(mount.sb().clone());
            }
        }
        found.sort_by_key(|sb| sb.id);
        found
    }

    /// Whether something in `path`'s namespace is mounted on its dentry: removing or renaming
    /// it is then `EBUSY` (`is_local_mountpoint`).
    pub(crate) fn is_local_mountpoint(&self, path: &Path) -> bool {
        if !path.dentry.is_mountpoint() {
            return false;
        }
        let state = self.tree.lock();
        state
            .ns_of(path.mnt.id)
            .is_some_and(|ns| state.is_local_mountpoint(ns, &path.dentry))
    }

    /// `detach_mounts`: lazily unmounts what other namespaces have mounted on `dentry`, whose
    /// name is gone.
    pub(crate) fn detach_mounts(&self, dentry: &Arc<Dentry>) {
        if !dentry.is_mountpoint() {
            return;
        }
        let _gone: Vec<_> = {
            let mut state = self.tree.lock();
            self.tree.seq.bump();
            state
                .mounted_on_dentry(dentry)
                .iter()
                .flat_map(|mount| state.umount_tree(mount, true))
                .collect()
        };
    }
}

/// `recalc_flags`.
fn recalc_flags(old: MountFlags, attr: &MountAttr) -> MountFlags {
    let pick = |old: bool, clear: bool, set: bool| (old && !clear) || set;
    let (set, clear) = (&attr.set, &attr.clear);
    MountFlags {
        read_only: pick(old.read_only, clear.read_only, set.read_only),
        nosuid: pick(old.nosuid, clear.nosuid, set.nosuid),
        nodev: pick(old.nodev, clear.nodev, set.nodev),
        noexec: pick(old.noexec, clear.noexec, set.noexec),
        noatime: pick(old.noatime, clear.noatime, set.noatime),
        nodiratime: pick(old.nodiratime, clear.nodiratime, set.nodiratime),
        strictatime: pick(old.strictatime, clear.strictatime, set.strictatime),
        nosymfollow: pick(old.nosymfollow, clear.nosymfollow, set.nosymfollow),
    }
}

impl MntNs {
    /// `copy_mnt_ns`: a new namespace holding a copy of every mount of this one, shared mounts
    /// joining their originals' peer groups and slaves their masters. Each of `paths` (the new
    /// root and working directory) moves to the copy of its mount.
    pub fn copy(&self, paths: &mut [&mut Path]) -> Arc<MntNs> {
        let mut state = self.tree.lock();
        let flags = CopyFlags {
            copy_unbindable: true,
            ..CopyFlags::default()
        };
        let source = self.root().mnt;
        let (root, copies) = state
            .copy_tree(&self.tree.next_id, &source, &source.root, flags)
            .expect("copying unbindable mounts too, a whole tree copies");
        let ns = MntNs::new(&self.tree, root);
        let copied: Vec<_> = copies.iter().map(|(_, copy)| copy.clone()).collect();
        state.set_ns(&copied, ns.id());
        for path in paths {
            if let Some((_, copy)) = copies
                .iter()
                .find(|(original, _)| Arc::ptr_eq(original, &path.mnt))
            {
                path.mnt = copy.clone();
            }
        }
        ns
    }

    /// The root a process entering the namespace gets: the namespace's root, below whatever
    /// is mounted on it (`mntns_install`'s `follow_down_one` loop).
    pub fn entry_root(&self) -> Path {
        let mut root = self.root();
        self.tree.follow_mounts(&mut root);
        root
    }
}
