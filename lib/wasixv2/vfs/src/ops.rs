//! Path operations with Linux's checks and error precedence (`fs/namei.c`, `fs/open.c`,
//! `fs/attr.c`, `fs/utimes.c`, `fs/xattr.c`, `fs/d_path.c`).

use std::sync::Arc;

use wasixv2_abi::{Errno, OpenFlags};

use crate::{
    Vfs,
    cred::{Cap, Cred, MAY_EXEC, MAY_READ, MAY_WRITE, generic_permission, sticky_denies},
    dentry::Dentry,
    file::File,
    inode::DirLock,
    mount::Path,
    namei::{Last, LookupFlags, Walk, lookup_one},
    notify::{FsEvent, Object},
    types::{
        Attr, Device, FileType, FsResult, MODE_PERM, NewKind, NewNode, OpenOptions, RenameMode,
        S_ISGID, S_ISUID, S_ISVTX, S_IXGRP, SetAttr, StatFs, TimeSet, XattrSet,
    },
};

/// Who is asking and where their root is: the per-call part of a process's filesystem context.
pub struct Ctx<'a> {
    pub cred: &'a Cred,
    pub root: &'a Path,
    pub umask: u32,
}

/// What to open (`struct open_how`, after `build_open_flags` validated it).
#[derive(Clone, Copy, Debug, Default)]
pub struct OpenHow {
    pub flags: OpenFlags,
    /// Permission bits for a new file.
    pub mode: u32,
    pub lookup: LookupFlags,
}

/// `XATTR_NAME_MAX` and `XATTR_SIZE_MAX`.
pub const XATTR_NAME_MAX: usize = 255;
pub const XATTR_SIZE_MAX: usize = 65536;

impl Vfs {
    /// Resolves `path` from `start` (`filename_lookup`).
    pub async fn lookup(
        &self,
        cx: &Ctx<'_>,
        start: &Path,
        path: &[u8],
        flags: LookupFlags,
    ) -> FsResult<Path> {
        if path.is_empty() && flags.empty {
            return Ok(start.clone());
        }
        Walk::new(self, cx.cred, cx.root, start, path, flags)?
            .resolve()
            .await
    }

    /// `inode_permission`: mode bits, plus no writes to a read-only filesystem.
    pub async fn permission(&self, cred: &Cred, path: &Path, mask: u32) -> FsResult<()> {
        let attr = path.dentry.node().getattr().await?;
        if mask & MAY_WRITE != 0
            && path.dentry.sb().info.read_only
            && matches!(
                attr.file_type,
                FileType::Regular | FileType::Directory | FileType::Symlink
            )
        {
            return Err(Errno::EROFS);
        }
        generic_permission(cred, &attr, mask)
    }

    /// `getattr` with the device filled in from the filesystem.
    pub async fn stat(&self, path: &Path) -> FsResult<Attr> {
        let mut attr = path.dentry.node().getattr().await?;
        let sb = path.dentry.sb();
        attr.dev = Some(match attr.dev {
            None => sb.dev,
            Some(dev) => self.sub_dev(sb, dev),
        });
        Ok(attr)
    }

    pub async fn statfs(&self, path: &Path) -> FsResult<StatFs> {
        path.dentry.sb().fs.statfs().await
    }

    fn object(dentry: &Dentry) -> Object {
        Object {
            sb: dentry.sb().id,
            ino: dentry.node().ino(),
        }
    }

    /// `do_sys_openat2` from the point where the flags are validated.
    pub async fn open(
        self: &Arc<Self>,
        cx: &Ctx<'_>,
        start: &Path,
        path: &[u8],
        how: &OpenHow,
    ) -> FsResult<File> {
        let flags = how.flags;
        if flags.contains(OpenFlags::O_PATH) {
            let path = self.lookup(cx, start, path, how.lookup).await?;
            return Ok(File::new(self.clone(), path, None, false, false, true));
        }
        let access = flags & OpenFlags::O_ACCMODE;
        let mut acc_mode = match access {
            OpenFlags::O_RDONLY => MAY_READ,
            OpenFlags::O_WRONLY => MAY_WRITE,
            _ => MAY_READ | MAY_WRITE,
        };
        if flags.contains(OpenFlags::O_TRUNC) {
            acc_mode |= MAY_WRITE;
        }
        if flags.contains(OpenFlags::__O_TMPFILE) {
            return self.open_tmpfile(cx, start, path, how, acc_mode).await;
        }

        let mut walk = Walk::new(self, cx.cred, cx.root, start, path, how.lookup)?;
        let creating = flags.contains(OpenFlags::O_CREAT);
        let mut created = false;
        let target = if path.is_empty() {
            start.clone()
        } else {
            loop {
                let last = walk.walk_parent().await?;
                if !creating {
                    if let Some(found) = walk.step_last(&last).await? {
                        break found;
                    }
                    continue;
                }
                // `open_last_lookups` with O_CREAT.
                let name = match last {
                    Last::Normal(name) => name,
                    _ => return Err(Errno::EISDIR),
                };
                if walk.flags.directory {
                    return Err(Errno::EISDIR);
                }
                // The parent stays locked from the lookup to the create, so O_CREAT|O_EXCL is
                // atomic whatever the driver does.
                let lock = walk.path.dentry.inode().lock().await;
                match walk.child_opt_locked(&name).await? {
                    Some(_) if flags.contains(OpenFlags::O_EXCL) => return Err(Errno::EEXIST),
                    Some(found) => {
                        drop(lock);
                        match walk.step_into(found).await? {
                            Some(found) => break found,
                            None => continue,
                        }
                    }
                    None => {
                        let dir = walk.path.clone();
                        let new = NewNode {
                            kind: NewKind::Regular,
                            ..self
                                .new_node_attrs(cx, &dir, how.mode & MODE_PERM, false)
                                .await?
                        };
                        match self.create(cx, &dir, &name, &new).await {
                            Ok(dentry) => {
                                created = true;
                                break Path {
                                    mnt: dir.mnt,
                                    dentry,
                                };
                            }
                            // Lost a race with another creator; open what it made.
                            Err(Errno::EEXIST) if !flags.contains(OpenFlags::O_EXCL) => continue,
                            Err(errno) => return Err(errno),
                        }
                    }
                }
            }
        };

        // `do_open`.
        let file_type = target.dentry.file_type();
        if creating && file_type == FileType::Directory {
            return Err(Errno::EISDIR);
        }
        if walk.flags.directory && file_type != FileType::Directory {
            return Err(Errno::ENOTDIR);
        }
        let mut truncate = false;
        if created {
            acc_mode = 0;
        } else if file_type == FileType::Regular && flags.contains(OpenFlags::O_TRUNC) {
            if target.mnt.is_read_only() {
                return Err(Errno::EROFS);
            }
            truncate = true;
        }
        self.may_open(cx.cred, &target, acc_mode, flags).await?;
        let writable = access != OpenFlags::O_RDONLY;
        if writable && target.mnt.is_read_only() && !created {
            return Err(Errno::EROFS);
        }
        self.finish_open(target, flags, writable, truncate).await
    }

    async fn finish_open(
        self: &Arc<Self>,
        target: Path,
        flags: OpenFlags,
        writable: bool,
        truncate: bool,
    ) -> FsResult<File> {
        let access = flags & OpenFlags::O_ACCMODE;
        let readable = access != OpenFlags::O_WRONLY;
        let handle = match target.dentry.file_type() {
            FileType::Regular | FileType::Directory => {
                let options = OpenOptions {
                    read: readable,
                    write: writable,
                    noatime: flags.contains(OpenFlags::O_NOATIME) || target.mnt.flags.noatime,
                    sync: flags.contains(OpenFlags::O_DSYNC),
                };
                target.dentry.node().open(options).await?
            }
            // TODO: FIFOs open a pipe once the kernel's pipes can be attached to an inode;
            // device nodes need drivers. Sockets are ENXIO on Linux too.
            _ => return Err(Errno::ENXIO),
        };
        if truncate {
            let attr = SetAttr {
                size: Some(0),
                mtime: Some(TimeSet::Now),
                ctime: Some(TimeSet::Now),
                ..SetAttr::default()
            };
            target.dentry.node().setattr(&attr).await?;
        }
        self.notify(FsEvent::Open {
            object: Self::object(&target.dentry),
        });
        Ok(File::new(
            self.clone(),
            target,
            Some(handle),
            readable,
            writable,
            false,
        ))
    }

    /// `may_open`.
    async fn may_open(
        &self,
        cred: &Cred,
        path: &Path,
        acc_mode: u32,
        flags: OpenFlags,
    ) -> FsResult<()> {
        match path.dentry.file_type() {
            FileType::Symlink => return Err(Errno::ELOOP),
            FileType::Directory => {
                if acc_mode & MAY_WRITE != 0 {
                    return Err(Errno::EISDIR);
                }
                if acc_mode & MAY_EXEC != 0 {
                    return Err(Errno::EACCES);
                }
            }
            FileType::CharDevice | FileType::BlockDevice if path.mnt.flags.nodev => {
                return Err(Errno::EACCES);
            }
            _ => {}
        }
        if acc_mode != 0 {
            self.permission(cred, path, acc_mode).await?;
        }
        if flags.contains(OpenFlags::O_NOATIME) {
            let attr = path.dentry.node().getattr().await?;
            if !cred.owns(&attr) {
                return Err(Errno::EPERM);
            }
        }
        Ok(())
    }

    /// `O_TMPFILE`: an unnamed regular file in the directory `path` names.
    async fn open_tmpfile(
        self: &Arc<Self>,
        cx: &Ctx<'_>,
        start: &Path,
        path: &[u8],
        how: &OpenHow,
        acc_mode: u32,
    ) -> FsResult<File> {
        let lookup = LookupFlags {
            directory: true,
            ..how.lookup
        };
        let dir = self.lookup(cx, start, path, lookup).await?;
        self.permission(cx.cred, &dir, MAY_WRITE | MAY_EXEC).await?;
        if dir.mnt.is_read_only() {
            return Err(Errno::EROFS);
        }
        let new = NewNode {
            kind: NewKind::Regular,
            ..self
                .new_node_attrs(cx, &dir, how.mode & MODE_PERM, false)
                .await?
        };
        let node = dir.dentry.node().create_unnamed(&new).await?;
        let target = Path {
            mnt: dir.mnt.clone(),
            dentry: Dentry::anonymous(dir.dentry.sb(), node),
        };
        target
            .dentry
            .inode()
            .set_linkable(!how.flags.contains(OpenFlags::O_EXCL));
        self.may_open(cx.cred, &target, acc_mode & !MAY_WRITE, how.flags)
            .await?;
        self.finish_open(target, how.flags, true, false).await
    }

    /// Ownership and permission bits of a new object in `dir`: the umask applied, the group from
    /// the directory if it is set-gid (`inode_init_owner`, `mode_strip_umask`).
    async fn new_node_attrs(
        &self,
        cx: &Ctx<'_>,
        dir: &Path,
        mode: u32,
        is_dir: bool,
    ) -> FsResult<NewNode<'static>> {
        let dir_attr = dir.dentry.node().getattr().await?;
        let mut mode = mode & !cx.umask;
        let gid = if dir_attr.mode & S_ISGID != 0 {
            if is_dir {
                mode |= S_ISGID;
            }
            dir_attr.gid
        } else {
            cx.cred.fsgid
        };
        if !is_dir && mode & S_ISGID != 0 && !cx.cred.in_group(gid) && !cx.cred.capable(Cap::Fsetid)
        {
            mode &= !S_ISGID;
        }
        Ok(NewNode {
            kind: NewKind::Regular,
            mode,
            uid: cx.cred.fsuid,
            gid,
        })
    }

    /// `may_create` and the driver call; `dir` is locked.
    async fn create(
        &self,
        cx: &Ctx<'_>,
        dir: &Path,
        name: &[u8],
        new: &NewNode<'_>,
    ) -> FsResult<Arc<Dentry>> {
        if dir.mnt.is_read_only() {
            return Err(Errno::EROFS);
        }
        self.may_create(cx.cred, dir).await?;
        if matches!(new.kind, NewKind::CharDevice(_) | NewKind::BlockDevice(_))
            && !cx.cred.capable(Cap::Mknod)
        {
            return Err(Errno::EPERM);
        }
        let node = dir.dentry.node().create(name, new).await?;
        let is_dir = node.file_type() == FileType::Directory;
        self.notify(FsEvent::Create {
            dir: Self::object(&dir.dentry),
            name,
            is_dir,
        });
        Ok(dir.dentry.child(name, node))
    }

    async fn may_create(&self, cred: &Cred, dir: &Path) -> FsResult<()> {
        self.permission(cred, dir, MAY_WRITE | MAY_EXEC).await?;
        if dir.dentry.is_unlinked() || dir.dentry.inode().is_dead() {
            return Err(Errno::ENOENT);
        }
        Ok(())
    }

    /// `filename_create`: the parent, locked, and the name of something about to be created,
    /// which does not exist.
    async fn prepare_create(
        &self,
        cx: &Ctx<'_>,
        start: &Path,
        path: &[u8],
        want_dir: bool,
    ) -> FsResult<(Path, Vec<u8>, DirLock)> {
        let mut walk = Walk::new(self, cx.cred, cx.root, start, path, LookupFlags::default())?;
        let last = walk.walk_parent().await?;
        let Last::Normal(name) = last else {
            return Err(Errno::EEXIST);
        };
        let trailing_slash = walk.flags.directory;
        let lock = walk.path.dentry.inode().lock().await;
        if walk.child_opt_locked(&name).await?.is_some() {
            return Err(Errno::EEXIST);
        }
        if trailing_slash && !want_dir {
            return Err(Errno::ENOENT);
        }
        Ok((walk.path, name, lock))
    }

    pub async fn mkdir(&self, cx: &Ctx<'_>, start: &Path, path: &[u8], mode: u32) -> FsResult<()> {
        let (dir, name, _lock) = self.prepare_create(cx, start, path, true).await?;
        let new = NewNode {
            kind: NewKind::Directory,
            ..self
                .new_node_attrs(cx, &dir, mode & (0o777 | S_ISVTX), true)
                .await?
        };
        self.create(cx, &dir, &name, &new).await.map(drop)
    }

    /// `mknodat`; `mode` includes the type bits.
    pub async fn mknod(
        &self,
        cx: &Ctx<'_>,
        start: &Path,
        path: &[u8],
        mode: u32,
        dev: Device,
    ) -> FsResult<()> {
        let kind = match FileType::from_mode(mode) {
            _ if mode & 0o170000 == 0 => NewKind::Regular,
            Some(FileType::Regular) => NewKind::Regular,
            Some(FileType::CharDevice) => NewKind::CharDevice(dev),
            Some(FileType::BlockDevice) => NewKind::BlockDevice(dev),
            Some(FileType::Fifo) => NewKind::Fifo,
            Some(FileType::Socket) => NewKind::Socket,
            Some(FileType::Directory) => return Err(Errno::EPERM),
            _ => return Err(Errno::EINVAL),
        };
        let (dir, name, _lock) = self.prepare_create(cx, start, path, false).await?;
        let new = NewNode {
            kind,
            ..self
                .new_node_attrs(cx, &dir, mode & MODE_PERM, false)
                .await?
        };
        self.create(cx, &dir, &name, &new).await.map(drop)
    }

    pub async fn symlink(
        &self,
        cx: &Ctx<'_>,
        target: &[u8],
        start: &Path,
        path: &[u8],
    ) -> FsResult<()> {
        if target.is_empty() {
            return Err(Errno::ENOENT);
        }
        let (dir, name, _lock) = self.prepare_create(cx, start, path, false).await?;
        let new = NewNode {
            kind: NewKind::Symlink(target),
            ..self.new_node_attrs(cx, &dir, 0o777, false).await?
        };
        // Symlinks are always 0777; the umask does not apply.
        let new = NewNode { mode: 0o777, ..new };
        self.create(cx, &dir, &name, &new).await.map(drop)
    }

    /// `linkat`: `old` is resolved with `old_flags`.
    pub async fn link(
        &self,
        cx: &Ctx<'_>,
        old_start: &Path,
        old: &[u8],
        old_flags: LookupFlags,
        new_start: &Path,
        new: &[u8],
    ) -> FsResult<()> {
        let source = self.lookup(cx, old_start, old, old_flags).await?;
        let (dir, name, _lock) = self.prepare_create(cx, new_start, new, false).await?;
        if !Arc::ptr_eq(&source.mnt, &dir.mnt) {
            return Err(Errno::EXDEV);
        }
        if dir.mnt.is_read_only() {
            return Err(Errno::EROFS);
        }
        self.may_create(cx.cred, &dir).await?;
        if source.dentry.file_type() == FileType::Directory {
            return Err(Errno::EPERM);
        }
        let attr = source.dentry.node().getattr().await?;
        // A removed file cannot get a name back, except an O_TMPFILE one (`I_LINKABLE`).
        let inode = source.dentry.inode();
        if attr.nlink == 0 && !inode.is_linkable() {
            return Err(Errno::ENOENT);
        }
        dir.dentry.node().link(&name, source.dentry.node()).await?;
        inode.set_linkable(false);
        self.notify(FsEvent::Create {
            dir: Self::object(&dir.dentry),
            name: &name,
            is_dir: false,
        });
        self.notify(FsEvent::Attrib {
            object: Self::object(&source.dentry),
        });
        dir.dentry.child(&name, source.dentry.node().clone());
        Ok(())
    }

    /// `may_delete_dentry` for `victim` in `dir`.
    async fn may_delete(
        &self,
        cred: &Cred,
        dir: &Path,
        victim: &Path,
        is_dir: bool,
    ) -> FsResult<()> {
        self.permission(cred, dir, MAY_WRITE | MAY_EXEC).await?;
        let dir_attr = dir.dentry.node().getattr().await?;
        let victim_attr = victim.dentry.node().getattr().await?;
        if sticky_denies(cred, &dir_attr, &victim_attr) {
            return Err(Errno::EPERM);
        }
        match (is_dir, victim.dentry.is_dir()) {
            (true, false) => return Err(Errno::ENOTDIR),
            (false, true) => return Err(Errno::EISDIR),
            _ => {}
        }
        if dir.dentry.is_unlinked() {
            return Err(Errno::ENOENT);
        }
        Ok(())
    }

    /// The parent walk shared by `unlink`, `rmdir` and `rename`: the parent path, the last
    /// component, and whether the path had a trailing slash.
    async fn parent_of(
        &self,
        cx: &Ctx<'_>,
        start: &Path,
        path: &[u8],
    ) -> FsResult<(Path, Last, bool)> {
        let mut walk = Walk::new(self, cx.cred, cx.root, start, path, LookupFlags::default())?;
        let last = walk.walk_parent().await?;
        Ok((walk.path.clone(), last, walk.flags.directory))
    }

    pub async fn unlink(&self, cx: &Ctx<'_>, start: &Path, path: &[u8]) -> FsResult<()> {
        let (dir, last, trailing_slash) = self.parent_of(cx, start, path).await?;
        let Last::Normal(name) = last else {
            return Err(Errno::EISDIR);
        };
        if dir.mnt.is_read_only() {
            return Err(Errno::EROFS);
        }
        let _lock = dir.dentry.inode().lock().await;
        let victim = lookup_one(&dir, &name).await?;
        if trailing_slash {
            return Err(if victim.dentry.is_dir() {
                Errno::EISDIR
            } else {
                Errno::ENOTDIR
            });
        }
        self.may_delete(cx.cred, &dir, &victim, false).await?;
        if victim.dentry.is_mountpoint() {
            return Err(Errno::EBUSY);
        }
        dir.dentry.node().unlink(&name).await?;
        victim.dentry.detach();
        self.notify(FsEvent::Delete {
            dir: Self::object(&dir.dentry),
            name: &name,
            is_dir: false,
            object: Self::object(&victim.dentry),
        });
        Ok(())
    }

    pub async fn rmdir(&self, cx: &Ctx<'_>, start: &Path, path: &[u8]) -> FsResult<()> {
        let (dir, last, _) = self.parent_of(cx, start, path).await?;
        let name = match last {
            Last::Normal(name) => name,
            Last::DotDot => return Err(Errno::ENOTEMPTY),
            Last::Dot => return Err(Errno::EINVAL),
            Last::Root => return Err(Errno::EBUSY),
        };
        if dir.mnt.is_read_only() {
            return Err(Errno::EROFS);
        }
        let _lock = dir.dentry.inode().lock().await;
        let victim = lookup_one(&dir, &name).await?;
        let _victim_lock = victim.dentry.inode().lock().await;
        self.may_delete(cx.cred, &dir, &victim, true).await?;
        if victim.dentry.is_mountpoint() {
            return Err(Errno::EBUSY);
        }
        dir.dentry.node().rmdir(&name).await?;
        victim.dentry.inode().set_dead();
        victim.dentry.detach();
        self.notify(FsEvent::Delete {
            dir: Self::object(&dir.dentry),
            name: &name,
            is_dir: true,
            object: Self::object(&victim.dentry),
        });
        Ok(())
    }

    /// `renameat2`.
    pub async fn rename(
        &self,
        cx: &Ctx<'_>,
        old_start: &Path,
        old: &[u8],
        new_start: &Path,
        new: &[u8],
        mode: RenameMode,
    ) -> FsResult<()> {
        let (old_dir, old_last, old_slash) = self.parent_of(cx, old_start, old).await?;
        let (new_dir, new_last, new_slash) = self.parent_of(cx, new_start, new).await?;
        if !Arc::ptr_eq(&old_dir.mnt, &new_dir.mnt) {
            return Err(Errno::EXDEV);
        }
        let Last::Normal(old_name) = old_last else {
            return Err(Errno::EBUSY);
        };
        let Last::Normal(new_name) = new_last else {
            return Err(if mode == RenameMode::NoReplace {
                Errno::EEXIST
            } else {
                Errno::EBUSY
            });
        };
        if old_dir.mnt.is_read_only() {
            return Err(Errno::EROFS);
        }
        let sb = old_dir.dentry.sb().clone();
        let cross_dir = !Arc::ptr_eq(old_dir.dentry.inode(), new_dir.dentry.inode());
        let _rename_guard = if cross_dir {
            Some(sb.rename_lock.lock().await)
        } else {
            None
        };
        let _locks = lock_rename(&old_dir, &new_dir).await;

        // `__start_renaming`.
        let source = lookup_one(&old_dir, &old_name).await?;
        let target = match lookup_one(&new_dir, &new_name).await {
            Ok(target) if mode == RenameMode::NoReplace => {
                drop(target);
                return Err(Errno::EEXIST);
            }
            Ok(target) => Some(target),
            Err(Errno::ENOENT) if mode != RenameMode::Exchange => None,
            Err(errno) => return Err(errno),
        };
        if cross_dir {
            // `lock_rename`'s trap: the child of one parent that is an ancestor of the other.
            if source.dentry.is_ancestor_of(&new_dir.dentry) {
                return Err(Errno::EINVAL);
            }
            if let Some(target) = &target
                && target.dentry.is_ancestor_of(&old_dir.dentry)
            {
                return Err(if mode == RenameMode::Exchange {
                    Errno::EINVAL
                } else {
                    Errno::ENOTEMPTY
                });
            }
        }
        if mode == RenameMode::Exchange
            && let Some(target) = &target
            && !target.dentry.is_dir()
            && new_slash
        {
            return Err(Errno::ENOTDIR);
        }
        if !source.dentry.is_dir() && (old_slash || (mode != RenameMode::Exchange && new_slash)) {
            return Err(Errno::ENOTDIR);
        }

        // `vfs_rename`.
        if let Some(target) = &target
            && target.dentry.node().ino() == source.dentry.node().ino()
        {
            return Ok(());
        }
        let is_dir = source.dentry.is_dir();
        self.may_delete(cx.cred, &old_dir, &source, is_dir).await?;
        let new_is_dir = target.as_ref().is_some_and(|t| t.dentry.is_dir());
        match &target {
            None => self.may_create(cx.cred, &new_dir).await?,
            Some(target) => {
                let as_dir = if mode == RenameMode::Exchange {
                    new_is_dir
                } else {
                    is_dir
                };
                self.may_delete(cx.cred, &new_dir, target, as_dir).await?;
            }
        }
        if cross_dir {
            if is_dir {
                self.permission(cx.cred, &source, MAY_WRITE).await?;
            }
            if mode == RenameMode::Exchange
                && let Some(target) = &target
                && new_is_dir
            {
                self.permission(cx.cred, target, MAY_WRITE).await?;
            }
        }
        if source.dentry.is_mountpoint()
            || target.as_ref().is_some_and(|t| t.dentry.is_mountpoint())
        {
            return Err(Errno::EBUSY);
        }
        // A directory being replaced is locked like rmdir's victim, so nothing is created in it.
        let replaced_dir = match &target {
            Some(target) if new_is_dir && mode != RenameMode::Exchange => Some(target),
            _ => None,
        };
        let _victim_lock = match replaced_dir {
            Some(target) => Some(target.dentry.inode().lock().await),
            None => None,
        };
        old_dir
            .dentry
            .node()
            .rename(&old_name, new_dir.dentry.node(), &new_name, mode)
            .await?;

        if mode == RenameMode::Exchange {
            if let Some(target) = &target {
                target.dentry.move_to(&old_dir.dentry, &old_name);
            }
        } else if let Some(target) = &target {
            if let Some(dir) = replaced_dir {
                dir.dentry.inode().set_dead();
            }
            target.dentry.detach();
        }
        source.dentry.move_to(&new_dir.dentry, &new_name);
        self.notify(FsEvent::Move {
            from_dir: Self::object(&old_dir.dentry),
            from_name: &old_name,
            to_dir: Self::object(&new_dir.dentry),
            to_name: &new_name,
            is_dir,
            object: Self::object(&source.dentry),
        });
        Ok(())
    }

    /// `readlinkat`: the target of the symlink `path` names.
    pub async fn readlink(
        &self,
        cx: &Ctx<'_>,
        start: &Path,
        path: &[u8],
        empty: bool,
    ) -> FsResult<Vec<u8>> {
        let flags = LookupFlags {
            empty,
            ..LookupFlags::nofollow()
        };
        let link = self.lookup(cx, start, path, flags).await?;
        if link.dentry.file_type() != FileType::Symlink {
            return Err(Errno::EINVAL);
        }
        link.dentry.node().readlink().await
    }

    /// `faccessat2`: `mask` is a `MAY_*` combination; `cx.cred` is already the right identity
    /// (real ids unless `AT_EACCESS`).
    pub async fn access(&self, cx: &Ctx<'_>, path: &Path, mask: u32) -> FsResult<()> {
        if mask == 0 {
            return Ok(());
        }
        if mask & MAY_WRITE != 0
            && path.mnt.is_read_only()
            && matches!(
                path.dentry.file_type(),
                FileType::Regular | FileType::Directory | FileType::Symlink
            )
        {
            return Err(Errno::EROFS);
        }
        self.permission(cx.cred, path, mask).await
    }

    async fn setattr(&self, path: &Path, attr: SetAttr) -> FsResult<()> {
        path.dentry.node().setattr(&attr).await?;
        self.notify(FsEvent::Attrib {
            object: Self::object(&path.dentry),
        });
        Ok(())
    }

    /// `chmod_common`.
    pub async fn chmod(&self, cx: &Ctx<'_>, path: &Path, mode: u32) -> FsResult<()> {
        if path.mnt.is_read_only() {
            return Err(Errno::EROFS);
        }
        let attr = path.dentry.node().getattr().await?;
        if !cx.cred.owns(&attr) {
            return Err(Errno::EPERM);
        }
        let mut mode = mode & MODE_PERM;
        // `setattr_prepare`: only members of the file's group may keep set-gid.
        if !cx.cred.in_group(attr.gid) && !cx.cred.capable(Cap::Fsetid) {
            mode &= !S_ISGID;
        }
        let set = SetAttr {
            mode: Some(mode),
            ctime: Some(TimeSet::Now),
            ..SetAttr::default()
        };
        self.setattr(path, set).await
    }

    /// `chown_common`; `None` leaves an id unchanged.
    pub async fn chown(
        &self,
        cx: &Ctx<'_>,
        path: &Path,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        if path.mnt.is_read_only() {
            return Err(Errno::EROFS);
        }
        let attr = path.dentry.node().getattr().await?;
        let cred = cx.cred;
        // `chown_ok` and `chgrp_ok`.
        if let Some(uid) = uid
            && uid != attr.uid
            && !cred.capable(Cap::Chown)
        {
            return Err(Errno::EPERM);
        }
        if let Some(gid) = gid
            && gid != attr.gid
            && !((cred.fsuid == attr.uid && cred.in_group(gid)) || cred.capable(Cap::Chown))
        {
            return Err(Errno::EPERM);
        }
        if uid.is_none() && gid.is_none() {
            return self.setattr(path, ctime_only()).await;
        }
        let mut set = SetAttr {
            uid,
            gid,
            ctime: Some(TimeSet::Now),
            ..SetAttr::default()
        };
        // ATTR_KILL_SUID / ATTR_KILL_SGID for non-directories.
        if attr.file_type != FileType::Directory {
            let mut mode = attr.mode & !S_ISUID;
            if attr.mode & S_IXGRP != 0 {
                mode &= !S_ISGID;
            }
            if mode != attr.mode {
                set.mode = Some(mode);
            }
        }
        self.setattr(path, set).await
    }

    /// `vfs_utimes`. `None` is `UTIME_OMIT`; both `Now` (or a NULL `times`) is the touch case
    /// that write permission allows.
    pub async fn utimes(
        &self,
        cx: &Ctx<'_>,
        path: &Path,
        atime: Option<TimeSet>,
        mtime: Option<TimeSet>,
    ) -> FsResult<()> {
        if path.mnt.is_read_only() {
            return Err(Errno::EROFS);
        }
        let attr = path.dentry.node().getattr().await?;
        let explicit =
            matches!(atime, Some(TimeSet::At(_))) || matches!(mtime, Some(TimeSet::At(_)));
        if !cx.cred.owns(&attr) {
            if explicit || atime.is_none() || mtime.is_none() {
                return Err(Errno::EPERM);
            }
            self.permission(cx.cred, path, MAY_WRITE).await?;
        }
        let set = SetAttr {
            atime,
            mtime,
            ctime: Some(TimeSet::Now),
            ..SetAttr::default()
        };
        self.setattr(path, set).await
    }

    /// `vfs_truncate` for `truncate(path)`; `ftruncate` checks the open file instead and calls
    /// `set_size`.
    pub async fn truncate(&self, cx: &Ctx<'_>, path: &Path, size: u64) -> FsResult<()> {
        match path.dentry.file_type() {
            FileType::Directory => return Err(Errno::EISDIR),
            FileType::Regular => {}
            _ => return Err(Errno::EINVAL),
        }
        if path.mnt.is_read_only() {
            return Err(Errno::EROFS);
        }
        self.permission(cx.cred, path, MAY_WRITE).await?;
        self.set_size(path, size).await
    }

    /// `do_truncate`: sets the size and stamps `mtime` and `ctime`.
    pub async fn set_size(&self, path: &Path, size: u64) -> FsResult<()> {
        let set = SetAttr {
            size: Some(size),
            mtime: Some(TimeSet::Now),
            ctime: Some(TimeSet::Now),
            ..SetAttr::default()
        };
        path.dentry.node().setattr(&set).await?;
        self.notify(FsEvent::Modify {
            object: Self::object(&path.dentry),
        });
        Ok(())
    }

    /// `d_path`: the path of `path` as seen from `root`. Unreachable locations (outside a
    /// chroot) get Linux's `(unreachable)` prefix; removed ones its ` (deleted)` suffix.
    pub fn d_path(&self, path: &Path, root: &Path) -> Vec<u8> {
        let mut parts: Vec<Vec<u8>> = Vec::new();
        let mut current = path.clone();
        let mut reachable = true;
        while current != *root {
            if current.is_mount_root() {
                match current.mnt.parent() {
                    Some(mountpoint) => {
                        current = mountpoint;
                        continue;
                    }
                    None => {
                        reachable = false;
                        break;
                    }
                }
            }
            match current.dentry.parent() {
                Some(parent) => {
                    parts.push(current.dentry.name());
                    current.dentry = parent;
                }
                None => {
                    reachable = false;
                    break;
                }
            }
        }
        let mut out = if reachable {
            Vec::new()
        } else {
            b"(unreachable)".to_vec()
        };
        for part in parts.iter().rev() {
            out.push(b'/');
            out.extend_from_slice(part);
        }
        if parts.is_empty() {
            out.push(b'/');
        }
        if path.dentry.is_unlinked() {
            out.extend_from_slice(b" (deleted)");
        }
        out
    }

    /// `xattr_permission` plus the namespace rules: which namespaces exist, and who may read or
    /// write them.
    async fn xattr_permission(
        &self,
        cx: &Ctx<'_>,
        path: &Path,
        name: &[u8],
        mask: u32,
    ) -> FsResult<()> {
        if name.is_empty() || name.len() > XATTR_NAME_MAX {
            return Err(Errno::ERANGE);
        }
        if mask & MAY_WRITE != 0 && path.mnt.is_read_only() {
            return Err(Errno::EROFS);
        }
        let denied = if mask & MAY_WRITE != 0 {
            Errno::EPERM
        } else {
            Errno::ENODATA
        };
        if name.starts_with(b"trusted.") {
            if !cx.cred.capable(Cap::SysAdmin) {
                return Err(denied);
            }
        } else if name.starts_with(b"user.") {
            let attr = path.dentry.node().getattr().await?;
            match attr.file_type {
                FileType::Regular => {}
                FileType::Directory => {
                    if attr.mode & S_ISVTX != 0 && mask & MAY_WRITE != 0 && !cx.cred.owns(&attr) {
                        return Err(Errno::EPERM);
                    }
                }
                _ => return Err(denied),
            }
        } else if !name.starts_with(b"security.") {
            // `system.` (POSIX ACLs) is not supported, and there are no other namespaces.
            return Err(Errno::EOPNOTSUPP);
        }
        if name.starts_with(b"security.") && mask & MAY_WRITE == 0 {
            return Ok(());
        }
        self.permission(cx.cred, path, mask).await
    }

    pub async fn getxattr(&self, cx: &Ctx<'_>, path: &Path, name: &[u8]) -> FsResult<Vec<u8>> {
        self.xattr_permission(cx, path, name, MAY_READ).await?;
        path.dentry.node().getxattr(name).await
    }

    pub async fn setxattr(
        &self,
        cx: &Ctx<'_>,
        path: &Path,
        name: &[u8],
        value: &[u8],
        how: XattrSet,
    ) -> FsResult<()> {
        if value.len() > XATTR_SIZE_MAX {
            return Err(Errno::E2BIG);
        }
        self.xattr_permission(cx, path, name, MAY_WRITE).await?;
        path.dentry.node().setxattr(name, value, how).await?;
        self.notify(FsEvent::Attrib {
            object: Self::object(&path.dentry),
        });
        Ok(())
    }

    pub async fn removexattr(&self, cx: &Ctx<'_>, path: &Path, name: &[u8]) -> FsResult<()> {
        self.xattr_permission(cx, path, name, MAY_WRITE).await?;
        path.dentry.node().removexattr(name).await?;
        self.notify(FsEvent::Attrib {
            object: Self::object(&path.dentry),
        });
        Ok(())
    }

    /// The attribute names `cx` may see, NUL-terminated and concatenated as `listxattr` returns
    /// them. Linux hides `trusted.` names from unprivileged callers.
    pub async fn listxattr(&self, cx: &Ctx<'_>, path: &Path) -> FsResult<Vec<u8>> {
        let names = match path.dentry.node().listxattr().await {
            Ok(names) => names,
            Err(Errno::EOPNOTSUPP) => Vec::new(),
            Err(errno) => return Err(errno),
        };
        let mut out = Vec::new();
        for name in names {
            if name.starts_with(b"trusted.") && !cx.cred.capable(Cap::SysAdmin) {
                continue;
            }
            out.extend_from_slice(&name);
            out.push(0);
        }
        Ok(out)
    }

    /// Creates each missing directory along `path`, as `mkdir -p` (for the embedder).
    pub async fn mkdir_all(&self, cx: &Ctx<'_>, path: &[u8]) -> FsResult<()> {
        let root = cx.root.clone();
        let mut prefix = Vec::new();
        for comp in path.split(|&b| b == b'/').filter(|c| !c.is_empty()) {
            prefix.push(b'/');
            prefix.extend_from_slice(comp);
            match self.mkdir(cx, &root, &prefix, 0o755).await {
                Ok(()) | Err(Errno::EEXIST) => {}
                Err(errno) => return Err(errno),
            }
        }
        Ok(())
    }
}

/// `lock_rename`: locks one parent, or both with the ancestor first and otherwise in a fixed
/// order, so renames and the parent-then-child locking of `rmdir` cannot deadlock. Cross-
/// directory callers hold the superblock's rename lock, which keeps the ancestry stable.
async fn lock_rename(old_dir: &Path, new_dir: &Path) -> (DirLock, Option<DirLock>) {
    let (a, b) = (old_dir.dentry.inode(), new_dir.dentry.inode());
    if Arc::ptr_eq(a, b) {
        return (a.lock().await, None);
    }
    let old_first = if old_dir.dentry.is_ancestor_of(&new_dir.dentry) {
        true
    } else if new_dir.dentry.is_ancestor_of(&old_dir.dentry) {
        false
    } else {
        a.lock_ptr() < b.lock_ptr()
    };
    let (first, second) = if old_first { (a, b) } else { (b, a) };
    let first = first.lock().await;
    (first, Some(second.lock().await))
}

fn ctime_only() -> SetAttr {
    SetAttr {
        ctime: Some(TimeSet::Now),
        ..SetAttr::default()
    }
}
