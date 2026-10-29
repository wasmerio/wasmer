//! Path operations with Linux's checks and error precedence (`fs/namei.c`, `fs/open.c`,
//! `fs/attr.c`, `fs/utimes.c`, `fs/xattr.c`, `fs/d_path.c`).

use std::sync::Arc;

use wasixv2_abi::{Errno, OpenFlags, SealFlags};

use crate::{
    Vfs,
    cred::{Cap, Cred, MAY_EXEC, MAY_READ, MAY_WRITE, sticky_denies},
    dentry::Dentry,
    driver::{Handle, Link, Node},
    file::File,
    inode::{DirLock, OpenAccess},
    locks::{Break, LeaseWait, LockKey},
    mount::{Mount, Path},
    namei::{Last, LookupFlags, Walk, lookup_one},
    notify::{FsEvent, Object, Perm},
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
    /// The calling task, as the kernel numbers it, for pseudo filesystems whose answers depend
    /// on who asks (`/proc/self`); 0 for the embedder, which is no task.
    pub task: u32,
    /// How the caller waits for leases its opens and truncations break; `None` for a caller
    /// that cannot wait (the embedder), which breaks none.
    pub leases: Option<&'a dyn LeaseWait>,
}

/// `__FMODE_EXEC`: an open flag only the kernel passes, for the program `execve` loads
/// (`do_open_execat`), which watches hear of as such (`FS_OPEN_EXEC`, `FS_OPEN_EXEC_PERM`).
pub const FMODE_EXEC: OpenFlags = OpenFlags(0x20);

/// How `finish_open` opens what the walk found.
#[derive(Clone, Copy)]
struct OpenMode {
    flags: OpenFlags,
    writable: bool,
    truncate: bool,
    /// `FMODE_NONOTIFY`: no permission or notification events, for the open or later.
    nonotify: bool,
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
        Walk::new(self, cx, start, path, flags)?.resolve().await
    }

    /// `inode_permission`: no writes to a read-only filesystem, then the node's own check for
    /// the caller `cx` ([`Node::permission`]), the mode bits unless its filesystem decides.
    pub async fn permission(&self, cx: &Ctx<'_>, path: &Path, mask: u32) -> FsResult<()> {
        let attr = path.dentry.node().getattr().await?;
        if mask & MAY_WRITE != 0
            && path.dentry.sb().is_read_only()
            && matches!(
                attr.file_type,
                FileType::Regular | FileType::Directory | FileType::Symlink
            )
        {
            return Err(Errno::EROFS);
        }
        // Nobody gets write access to an immutable file.
        if mask & MAY_WRITE != 0 && immutable(&path.dentry) {
            return Err(Errno::EPERM);
        }
        path.dentry.node().permission(cx.cred, cx.task, &attr, mask)
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

    pub(crate) fn object(dentry: &Dentry) -> Object {
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
            return Ok(File::new(
                self.clone(),
                path,
                None,
                false,
                None,
                false,
                true,
            ));
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

        let mut walk = Walk::new(self, cx, start, path, how.lookup)?;
        let creating = flags.contains(OpenFlags::O_CREAT);
        let excl = flags.contains(OpenFlags::O_EXCL);
        let mut created = None;
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
                    // A magic link's object exists; `do_open` treats it as found.
                    Last::Bind => break walk.path.clone(),
                    _ => return Err(Errno::EISDIR),
                };
                if walk.flags.directory {
                    return Err(Errno::EISDIR);
                }
                // The parent stays locked from the lookup to the create, so O_CREAT|O_EXCL is
                // atomic whatever the driver does.
                let lock = walk.path.dentry.inode().lock().await;
                // `lookup_open`: the existing object, or a new one.
                let found = loop {
                    if let Some(found) = walk.child_opt_locked(&name).await? {
                        if excl {
                            return Err(Errno::EEXIST);
                        }
                        break found;
                    }
                    let dir = walk.path.clone();
                    let new = NewNode {
                        kind: NewKind::Regular,
                        ..self
                            .new_node_attrs(cx, &dir, how.mode & MODE_PERM, false)
                            .await?
                    };
                    let options = open_options(flags, &dir, cx.task);
                    match self.create_open(cx, &dir, &name, &new, options).await {
                        Ok((dentry, handle)) => {
                            created = Some(handle);
                            break Path {
                                mnt: dir.mnt,
                                dentry,
                            };
                        }
                        // Created behind the VFS (on the host side of a host directory); open
                        // what is there now.
                        Err(Errno::EEXIST) if !excl => {}
                        Err(errno) => return Err(errno),
                    }
                };
                if created.is_some() {
                    break found;
                }
                drop(lock);
                match walk.step_into(found).await? {
                    Some(found) => break found,
                    None => continue,
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
        if created.is_some() {
            acc_mode = 0;
        } else if file_type == FileType::Regular && flags.contains(OpenFlags::O_TRUNC) {
            if target.mnt.is_read_only() {
                return Err(Errno::EROFS);
            }
            truncate = true;
        }
        self.may_open(cx, &target, acc_mode, flags).await?;
        let writable = access != OpenFlags::O_RDONLY;
        let mode = OpenMode {
            flags,
            writable,
            truncate,
            nonotify: false,
        };
        self.finish_open(cx, target, mode, created).await
    }

    /// Opens `target` for the caller, or takes the handle its creation returned, and builds the
    /// open file (`do_dentry_open`).
    async fn finish_open(
        self: &Arc<Self>,
        cx: &Ctx<'_>,
        target: Path,
        mode: OpenMode,
        handle: Option<Box<dyn Handle>>,
    ) -> FsResult<File> {
        let OpenMode {
            flags,
            writable,
            truncate,
            nonotify,
        } = mode;
        let task = cx.task;
        let access = flags & OpenFlags::O_ACCMODE;
        let readable = access != OpenFlags::O_WRONLY;
        // `do_dentry_open` takes write access only for what a read-only mount protects
        // (`special_file`): a FIFO or device node is not written through the filesystem.
        let special = matches!(
            target.dentry.file_type(),
            FileType::Fifo | FileType::CharDevice | FileType::BlockDevice | FileType::Socket
        );
        let write = match writable && !special {
            true => Some(self.tree.get_write_access(&target.mnt)?),
            false => None,
        };
        let inode = target.dentry.inode();
        let counted = match (write.is_some(), readable && !writable) {
            (true, _) => Some(OpenAccess::new(inode, true)),
            (false, true) => Some(OpenAccess::new(inode, false)),
            (false, false) => None,
        };
        // A file this open made has no leases.
        if handle.is_none() && target.dentry.file_type() == FileType::Regular {
            let nonblock = flags.contains(OpenFlags::O_NONBLOCK);
            self.break_lease(cx, Self::lock_key(&target.dentry), writable, nonblock)
                .await?;
        }
        let handle = match (handle, target.dentry.file_type()) {
            (Some(handle), _) => Some(handle),
            (None, FileType::Regular | FileType::Directory) => {
                let options = OpenOptions {
                    write: writable,
                    truncate,
                    ..open_options(flags, &target, task)
                };
                Some(target.dentry.node().open(options).await?)
            }
            // The kernel attaches the pipe (`fifo_open`) through `Inode::bound_or_bind`, or the
            // device driver its number names (`chrdev_open`).
            (None, FileType::Fifo | FileType::CharDevice | FileType::BlockDevice) => None,
            // `sock_no_open`.
            (None, FileType::Socket) => return Err(Errno::ENXIO),
            // `may_open` refused it already.
            (None, FileType::Symlink) => return Err(Errno::ELOOP),
        };
        let exec = flags.contains(FMODE_EXEC);
        if !nonotify {
            let perm = if exec { Perm::OpenExec } else { Perm::Open };
            self.permission_event(cx, &target, perm).await?;
        }
        if truncate {
            let attr = SetAttr {
                size: Some(0),
                mtime: Some(TimeSet::Now),
                ctime: Some(TimeSet::Now),
                ..SetAttr::default()
            };
            Self::notify_change(&target.dentry, &attr).await?;
        }
        if !nonotify {
            let object = Self::object(&target.dentry);
            self.notify_path(&target, FsEvent::Open { object, exec });
        }
        let file = File::new(
            self.clone(),
            target,
            handle,
            readable,
            write,
            writable,
            false,
        );
        Ok(file.with_access(counted).with_nonotify(nonotify))
    }

    /// `dentry_open_nonotify`: opens the object `path` names as the caller `cx`, with no
    /// lookup and no permission check, and without telling the watches anything about the
    /// open or about anything done through it (`FMODE_NONOTIFY`), as fanotify opens the
    /// descriptors of its events.
    pub async fn dentry_open_nonotify(
        self: &Arc<Self>,
        cx: &Ctx<'_>,
        path: Path,
        flags: OpenFlags,
    ) -> FsResult<File> {
        if path.dentry.file_type() == FileType::Symlink {
            return Err(Errno::ELOOP);
        }
        let access = flags & OpenFlags::O_ACCMODE;
        let mode = OpenMode {
            flags,
            writable: access != OpenFlags::O_RDONLY,
            truncate: false,
            nonotify: true,
        };
        self.finish_open(cx, path, mode, None).await
    }

    /// `break_lease`: an open of the regular file `key` names, for writing with `write`, or a
    /// truncation of it, tells the holders of the leases in its way and waits until they gave
    /// way or their time was up; `EWOULDBLOCK` instead with `nonblock` (`O_NONBLOCK`). A
    /// caller that cannot wait ([`Ctx::leases`]) breaks no leases.
    async fn break_lease(
        &self,
        cx: &Ctx<'_>,
        key: LockKey,
        write: bool,
        nonblock: bool,
    ) -> FsResult<()> {
        let Some(wait) = cx.leases else {
            return Ok(());
        };
        if !self.locks.any_leases() {
            return Ok(());
        }
        loop {
            match self
                .locks
                .break_leases(key, write, wait.now(), wait.break_time())
            {
                Break::Done => return Ok(()),
                Break::Wait { .. } if nonblock => return Err(Errno::EWOULDBLOCK),
                Break::Wait {
                    deadline,
                    generation,
                } => {
                    let changed = Box::pin(self.locks.changed_since(key, generation));
                    wait.wait(changed, deadline).await?;
                }
            }
        }
    }

    fn lock_key(dentry: &Dentry) -> LockKey {
        (dentry.sb().id, dentry.node().ino())
    }

    /// `may_open`.
    async fn may_open(
        &self,
        cx: &Ctx<'_>,
        path: &Path,
        acc_mode: u32,
        flags: OpenFlags,
    ) -> FsResult<()> {
        let cred = cx.cred;
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
            FileType::CharDevice | FileType::BlockDevice if path.mnt.flags().nodev => {
                return Err(Errno::EACCES);
            }
            _ => {}
        }
        if acc_mode != 0 {
            self.permission(cx, path, acc_mode).await?;
        }
        // An append-only file is opened for writing only to append.
        if append_only(&path.dentry) {
            let access = flags & OpenFlags::O_ACCMODE;
            if access != OpenFlags::O_RDONLY && !flags.contains(OpenFlags::O_APPEND) {
                return Err(Errno::EPERM);
            }
            if flags.contains(OpenFlags::O_TRUNC) {
                return Err(Errno::EPERM);
            }
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
        self.permission(cx, &dir, MAY_WRITE | MAY_EXEC).await?;
        // Held while the file is made (`mnt_want_write` in `do_o_tmpfile`); the open takes its
        // own.
        let _write = self.tree.get_write_access(&dir.mnt)?;
        let new = NewNode {
            kind: NewKind::Regular,
            ..self
                .new_node_attrs(cx, &dir, how.mode & MODE_PERM, false)
                .await?
        };
        let options = OpenOptions {
            write: true,
            ..open_options(how.flags, &dir, cx.task)
        };
        let (node, handle) = dir.dentry.node().create_unnamed(&new, options).await?;
        let target = Path {
            mnt: dir.mnt.clone(),
            dentry: Dentry::anonymous(dir.dentry.sb(), node),
        };
        target
            .dentry
            .inode()
            .set_linkable(!how.flags.contains(OpenFlags::O_EXCL));
        self.may_open(cx, &target, acc_mode & !MAY_WRITE, how.flags)
            .await?;
        let mode = OpenMode {
            flags: how.flags,
            writable: true,
            truncate: false,
            nonotify: false,
        };
        self.finish_open(cx, target, mode, Some(handle)).await
    }

    /// A new unnamed regular file on `mnt`, a mount outside the tree, open for reading and
    /// writing (`shmem_file_setup`, `alloc_file_pseudo`). `new` is taken as given: no umask, no
    /// permission check. `d_path` shows the file as `/<name> (deleted)`. It starts with `seals`
    /// where its filesystem supports seals.
    pub async fn open_unnamed(
        self: &Arc<Self>,
        mnt: &Arc<Mount>,
        name: &[u8],
        new: &NewNode<'_>,
        seals: SealFlags,
    ) -> FsResult<File> {
        let options = OpenOptions {
            read: true,
            write: true,
            noatime: false,
            sync: false,
            truncate: false,
            task: 0,
        };
        let write = self.tree.get_write_access(mnt)?;
        let (node, handle) = mnt.root.node().create_unnamed(new, options).await?;
        let path = mnt.pseudo_path(node, name);
        path.dentry.inode().init_seals(seals);
        let access = OpenAccess::new(path.dentry.inode(), true);
        Ok(File::new(
            self.clone(),
            path,
            Some(handle),
            true,
            Some(write),
            true,
            false,
        )
        .with_access(Some(access)))
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
        // `mode_strip_sgid`: `CAP_FSETID` over the directory.
        if !is_dir && mode & S_ISGID != 0 && !cx.cred.in_group_or_capable(&dir_attr, gid) {
            mode &= !S_ISGID;
        }
        Ok(NewNode {
            kind: NewKind::Regular,
            mode,
            uid: cx.cred.fsuid,
            gid,
            task: cx.task,
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
        self.may_create_node(cx, dir, new).await?;
        let node = dir.dentry.node().create(name, new).await?;
        Ok(self.created(dir, name, node))
    }

    /// `create` of a regular file that is opened at once (`O_CREAT`); `dir` is locked.
    async fn create_open(
        &self,
        cx: &Ctx<'_>,
        dir: &Path,
        name: &[u8],
        new: &NewNode<'_>,
        options: OpenOptions,
    ) -> FsResult<(Arc<Dentry>, Box<dyn Handle>)> {
        self.may_create_node(cx, dir, new).await?;
        let (node, handle) = dir.dentry.node().create_open(name, new, options).await?;
        Ok((self.created(dir, name, node), handle))
    }

    async fn may_create_node(&self, cx: &Ctx<'_>, dir: &Path, new: &NewNode<'_>) -> FsResult<()> {
        if dir.mnt.is_read_only() {
            return Err(Errno::EROFS);
        }
        self.may_create(cx, dir).await?;
        // `vfs_mknod`: `capable(CAP_MKNOD)`, which no user namespace's root has.
        if matches!(new.kind, NewKind::CharDevice(_) | NewKind::BlockDevice(_))
            && !cx.cred.capable_init(Cap::Mknod)
        {
            return Err(Errno::EPERM);
        }
        Ok(())
    }

    /// Reports `node`, just created as `name` in `dir`, and caches its name.
    fn created(&self, dir: &Path, name: &[u8], node: Arc<dyn Node>) -> Arc<Dentry> {
        let object = Object {
            sb: dir.dentry.sb().id,
            ino: node.ino(),
        };
        self.notify(FsEvent::Create {
            dir: Self::object(&dir.dentry),
            name,
            is_dir: node.file_type() == FileType::Directory,
            object,
        });
        dir.dentry.child(name, node)
    }

    async fn may_create(&self, cx: &Ctx<'_>, dir: &Path) -> FsResult<()> {
        let cred = cx.cred;
        // `fsuidgid_has_mapping`: what is made is owned by ids the filesystem's user namespace
        // can name.
        let owner = &dir.dentry.sb().owner;
        if !owner.has_uid(cred.fsuid) || !owner.has_gid(cred.fsgid) {
            return Err(Errno::EOVERFLOW);
        }
        self.permission(cx, dir, MAY_WRITE | MAY_EXEC).await?;
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
        let mut walk = Walk::new(self, cx, start, path, LookupFlags::default())?;
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
        self.may_create(cx, &dir).await?;
        // A link to an append-only or immutable file cannot be made.
        if append_only(&source.dentry) || immutable(&source.dentry) {
            return Err(Errno::EPERM);
        }
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
            object: Self::object(&source.dentry),
        });
        self.notify_object(
            &source.dentry,
            FsEvent::Attrib {
                object: Self::object(&source.dentry),
            },
        );
        dir.dentry.child(&name, source.dentry.node().clone());
        Ok(())
    }

    /// `may_delete_dentry` for `victim` in `dir`.
    async fn may_delete(
        &self,
        cx: &Ctx<'_>,
        dir: &Path,
        victim: &Path,
        is_dir: bool,
    ) -> FsResult<()> {
        let cred = cx.cred;
        self.permission(cx, dir, MAY_WRITE | MAY_EXEC).await?;
        if append_only(&dir.dentry) {
            return Err(Errno::EPERM);
        }
        let dir_attr = dir.dentry.node().getattr().await?;
        let victim_attr = victim.dentry.node().getattr().await?;
        if sticky_denies(cred, &dir_attr, &victim_attr)
            || append_only(&victim.dentry)
            || immutable(&victim.dentry)
        {
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
        let mut walk = Walk::new(self, cx, start, path, LookupFlags::default())?;
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
        self.may_delete(cx, &dir, &victim, false).await?;
        if self.is_local_mountpoint(&victim) {
            return Err(Errno::EBUSY);
        }
        dir.dentry.node().unlink(&name).await?;
        victim.dentry.detach();
        self.detach_mounts(&victim.dentry);
        let gone = victim
            .dentry
            .node()
            .getattr()
            .await
            .map_or(true, |attr| attr.nlink == 0);
        self.notify(FsEvent::Delete {
            dir: Self::object(&dir.dentry),
            name: &name,
            is_dir: false,
            object: Self::object(&victim.dentry),
            gone,
        });
        Ok(())
    }

    pub async fn rmdir(&self, cx: &Ctx<'_>, start: &Path, path: &[u8]) -> FsResult<()> {
        let (dir, last, _) = self.parent_of(cx, start, path).await?;
        let name = match last {
            Last::Normal(name) => name,
            Last::DotDot => return Err(Errno::ENOTEMPTY),
            Last::Dot => return Err(Errno::EINVAL),
            Last::Root | Last::Bind => return Err(Errno::EBUSY),
        };
        if dir.mnt.is_read_only() {
            return Err(Errno::EROFS);
        }
        let _lock = dir.dentry.inode().lock().await;
        let victim = lookup_one(&dir, &name).await?;
        let _victim_lock = victim.dentry.inode().lock().await;
        self.may_delete(cx, &dir, &victim, true).await?;
        if self.is_local_mountpoint(&victim) {
            return Err(Errno::EBUSY);
        }
        dir.dentry.node().rmdir(&name).await?;
        victim.dentry.inode().set_dead();
        victim.dentry.detach();
        self.detach_mounts(&victim.dentry);
        self.notify(FsEvent::Delete {
            dir: Self::object(&dir.dentry),
            name: &name,
            is_dir: true,
            object: Self::object(&victim.dentry),
            gone: true,
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
        if mode == RenameMode::Whiteout && !cx.cred.capable_init(Cap::Mknod) {
            return Err(Errno::EPERM);
        }
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
        self.may_delete(cx, &old_dir, &source, is_dir).await?;
        let new_is_dir = target.as_ref().is_some_and(|t| t.dentry.is_dir());
        match &target {
            None => self.may_create(cx, &new_dir).await?,
            Some(target) => {
                let as_dir = if mode == RenameMode::Exchange {
                    new_is_dir
                } else {
                    is_dir
                };
                self.may_delete(cx, &new_dir, target, as_dir).await?;
            }
        }
        if cross_dir {
            if is_dir {
                self.permission(cx, &source, MAY_WRITE).await?;
            }
            if mode == RenameMode::Exchange
                && let Some(target) = &target
                && new_is_dir
            {
                self.permission(cx, target, MAY_WRITE).await?;
            }
        }
        if self.is_local_mountpoint(&source)
            || target.as_ref().is_some_and(|t| self.is_local_mountpoint(t))
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

        match &target {
            Some(target) if mode == RenameMode::Exchange => {
                Dentry::exchange(&source.dentry, &target.dentry);
            }
            _ => {
                if let Some(target) = &target {
                    if let Some(dir) = replaced_dir {
                        dir.dentry.inode().set_dead();
                    }
                    target.dentry.detach();
                    self.detach_mounts(&target.dentry);
                }
                source.dentry.move_to(&new_dir.dentry, &new_name);
            }
        }
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
        // A magic link reads as where its object is (`proc_pid_readlink`).
        Ok(match link.dentry.node().get_link(cx).await? {
            Link::Body(body) => body,
            Link::Jump(target) => self.d_path(&target, cx.root),
        })
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
        self.permission(cx, path, mask).await
    }

    async fn setattr(&self, path: &Path, attr: SetAttr) -> FsResult<()> {
        Self::notify_change(&path.dentry, &attr).await?;
        self.notify_object(
            &path.dentry,
            FsEvent::Attrib {
                object: Self::object(&path.dentry),
            },
        );
        Ok(())
    }

    /// `notify_change` from the point where the caller's checks passed: the file's seals, then
    /// the driver.
    async fn notify_change(dentry: &Dentry, attr: &SetAttr) -> FsResult<()> {
        let inode = dentry.inode();
        let _sealed = inode.seal_lock().await;
        inode.sealed_setattr(attr).await?;
        dentry.node().setattr(attr).await?;
        Ok(())
    }

    /// `chmod_common`.
    pub async fn chmod(&self, cx: &Ctx<'_>, path: &Path, mode: u32) -> FsResult<()> {
        if path.mnt.is_read_only() {
            return Err(Errno::EROFS);
        }
        may_setattr(&path.dentry)?;
        let attr = path.dentry.node().getattr().await?;
        if !cx.cred.owns(&attr) {
            return Err(Errno::EPERM);
        }
        let mut mode = mode & MODE_PERM;
        // `setattr_prepare`: only members of the file's group, or `CAP_FSETID` over the file,
        // may keep set-gid.
        if !cx.cred.in_group_or_capable(&attr, attr.gid) {
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
        if uid.is_some() || gid.is_some() {
            may_setattr(&path.dentry)?;
        }
        let attr = path.dentry.node().getattr().await?;
        let cred = cx.cred;
        // `chown_ok` and `chgrp_ok`: only the owner may even "change" to the current ids.
        let owner = cred.fsuid == attr.uid;
        if let Some(uid) = uid
            && !(owner && uid == attr.uid)
            && !cred.capable_wrt(&attr, Cap::Chown)
        {
            return Err(Errno::EPERM);
        }
        if let Some(gid) = gid
            && !(owner && (gid == attr.gid || cred.in_group(gid)))
            && !cred.capable_wrt(&attr, Cap::Chown)
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
        let explicit =
            matches!(atime, Some(TimeSet::At(_))) || matches!(mtime, Some(TimeSet::At(_)));
        // `may_setattr`: set times on neither, the current time not on an immutable file.
        match explicit || atime.is_none() || mtime.is_none() {
            true => may_setattr(&path.dentry)?,
            false if immutable(&path.dentry) => return Err(Errno::EPERM),
            false => {}
        }
        let attr = path.dentry.node().getattr().await?;
        if !cx.cred.owns(&attr) {
            if explicit || atime.is_none() || mtime.is_none() {
                return Err(Errno::EPERM);
            }
            self.permission(cx, path, MAY_WRITE).await?;
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
        self.permission(cx, path, MAY_WRITE).await?;
        if append_only(&path.dentry) {
            return Err(Errno::EPERM);
        }
        // Write access while it truncates, so that no read lease is taken meanwhile.
        let _write = OpenAccess::new(path.dentry.inode(), true);
        self.break_lease(cx, Self::lock_key(&path.dentry), true, false)
            .await?;
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
        Self::notify_change(&path.dentry, &set).await?;
        let object = Self::object(&path.dentry);
        self.notify_path(path, FsEvent::Modify { object });
        Ok(())
    }

    /// `d_path`: the path of `path` as seen from `root`. Unreachable locations (outside a
    /// chroot) get Linux's `(unreachable)` prefix; removed ones its ` (deleted)` suffix. An
    /// object of an internal mount is `/<name> (deleted)`.
    pub fn d_path(&self, path: &Path, root: &Path) -> Vec<u8> {
        if !path.is_mount_root() {
            let fs = &path.dentry.sb().fs;
            if let Some(name) = fs.dname(&path.dentry.name(), path.dentry.node().as_ref()) {
                return name;
            }
        }
        // `simple_dname`, the `d_dname` of every pseudo dentry.
        if path.dentry.is_pseudo() {
            let mut out = b"/".to_vec();
            out.extend_from_slice(&path.dentry.name());
            out.extend_from_slice(b" (deleted)");
            return out;
        }
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
        // `may_write_xattr`.
        if mask & MAY_WRITE != 0 && (immutable(&path.dentry) || append_only(&path.dentry)) {
            return Err(Errno::EPERM);
        }
        let denied = if mask & MAY_WRITE != 0 {
            Errno::EPERM
        } else {
            Errno::ENODATA
        };
        if name.starts_with(b"trusted.") {
            if !cx.cred.capable_init(Cap::SysAdmin) {
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
        self.permission(cx, path, mask).await
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
        self.notify_object(
            &path.dentry,
            FsEvent::Attrib {
                object: Self::object(&path.dentry),
            },
        );
        Ok(())
    }

    pub async fn removexattr(&self, cx: &Ctx<'_>, path: &Path, name: &[u8]) -> FsResult<()> {
        self.xattr_permission(cx, path, name, MAY_WRITE).await?;
        path.dentry.node().removexattr(name).await?;
        self.notify_object(
            &path.dentry,
            FsEvent::Attrib {
                object: Self::object(&path.dentry),
            },
        );
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
            if name.starts_with(b"trusted.") && !cx.cred.capable_init(Cap::SysAdmin) {
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

/// The data-access options of an open by `task` with `flags` on `at`'s mount.
fn open_options(flags: OpenFlags, at: &Path, task: u32) -> OpenOptions {
    let access = flags & OpenFlags::O_ACCMODE;
    OpenOptions {
        read: access != OpenFlags::O_WRONLY,
        write: access != OpenFlags::O_RDONLY,
        noatime: flags.contains(OpenFlags::O_NOATIME) || at.mnt.flags().noatime,
        sync: flags.contains(OpenFlags::O_DSYNC),
        truncate: false,
        task,
    }
}

/// `IS_IMMUTABLE`.
pub(crate) fn immutable(dentry: &Dentry) -> bool {
    dentry.node().inode_flags() & crate::FS_IMMUTABLE_FL != 0
}

/// `IS_APPEND`.
pub(crate) fn append_only(dentry: &Dentry) -> bool {
    dentry.node().inode_flags() & crate::FS_APPEND_FL != 0
}

/// `may_setattr` for a change of mode, owner or explicit times.
fn may_setattr(dentry: &Dentry) -> FsResult<()> {
    match immutable(dentry) || append_only(dentry) {
        true => Err(Errno::EPERM),
        false => Ok(()),
    }
}

fn ctime_only() -> SetAttr {
    SetAttr {
        ctime: Some(TimeSet::Now),
        ..SetAttr::default()
    }
}
