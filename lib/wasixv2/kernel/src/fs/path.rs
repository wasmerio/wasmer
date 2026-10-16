//! Namespace syscalls (`fs/namei.c`) and the working directory, root and umask (`fs/open.c`,
//! `fs/d_path.c`).

use wasixv2_abi::{AtFlags, Buf, Cstr, RenameFlags};
use wasixv2_syscalls::{Caller, Errno, Fd, SysResult, UserSliceMut};
use wasixv2_vfs::{Cap, Device, LookupFlags, MAY_EXEC, RenameMode};

use super::read_path;
use crate::Kernel;

impl Kernel {
    pub(crate) async fn sys_mkdirat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        pathname: Cstr,
        mode: u32,
    ) -> SysResult<i32> {
        let path = read_path(caller.memory(), pathname).await?;
        let env = self.env(caller);
        let start = self.start(caller, &env, dfd, &path)?;
        self.vfs.mkdir(&env.cx(), &start, &path, mode).await?;
        Ok(0)
    }

    pub(crate) async fn sys_mknodat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        mode: u32,
        dev: u32,
    ) -> SysResult<i32> {
        let path = read_path(caller.memory(), filename).await?;
        let env = self.env(caller);
        let start = self.start(caller, &env, dfd, &path)?;
        self.vfs
            .mknod(&env.cx(), &start, &path, mode, Device::from_dev_t(dev))
            .await?;
        Ok(0)
    }

    pub(crate) async fn sys_unlinkat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        pathname: Cstr,
        flags: AtFlags,
    ) -> SysResult<i32> {
        if !(flags & !AtFlags::AT_REMOVEDIR).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        let path = read_path(caller.memory(), pathname).await?;
        let env = self.env(caller);
        let start = self.start(caller, &env, dfd, &path)?;
        if flags.contains(AtFlags::AT_REMOVEDIR) {
            self.vfs.rmdir(&env.cx(), &start, &path).await?;
        } else {
            self.vfs.unlink(&env.cx(), &start, &path).await?;
        }
        Ok(0)
    }

    pub(crate) async fn sys_symlinkat(
        &self,
        caller: &Caller<'_>,
        oldname: Cstr,
        newdfd: Fd,
        newname: Cstr,
    ) -> SysResult<i32> {
        let target = read_path(caller.memory(), oldname).await?;
        let path = read_path(caller.memory(), newname).await?;
        let env = self.env(caller);
        let start = self.start(caller, &env, newdfd, &path)?;
        self.vfs.symlink(&env.cx(), &target, &start, &path).await?;
        Ok(0)
    }

    pub(crate) async fn sys_linkat(
        &self,
        caller: &Caller<'_>,
        olddfd: Fd,
        oldname: Cstr,
        newdfd: Fd,
        newname: Cstr,
        flags: AtFlags,
    ) -> SysResult<i32> {
        if !(flags & !(AtFlags::AT_SYMLINK_FOLLOW | AtFlags::AT_EMPTY_PATH)).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        let old = read_path(caller.memory(), oldname).await?;
        let new = read_path(caller.memory(), newname).await?;
        let env = self.env(caller);
        let empty = flags.contains(AtFlags::AT_EMPTY_PATH);
        let old_start = if old.is_empty() && empty {
            self.start_or_fd(caller, &env, olddfd)?
        } else {
            self.start(caller, &env, olddfd, &old)?
        };
        let new_start = self.start(caller, &env, newdfd, &new)?;
        let old_flags = LookupFlags {
            follow: flags.contains(AtFlags::AT_SYMLINK_FOLLOW),
            empty,
            ..LookupFlags::default()
        };
        self.vfs
            .link(&env.cx(), &old_start, &old, old_flags, &new_start, &new)
            .await?;
        Ok(0)
    }

    pub(crate) async fn sys_renameat2(
        &self,
        caller: &Caller<'_>,
        olddfd: Fd,
        oldname: Cstr,
        newdfd: Fd,
        newname: Cstr,
        flags: RenameFlags,
    ) -> SysResult<i32> {
        let known = RenameFlags::RENAME_NOREPLACE
            | RenameFlags::RENAME_EXCHANGE
            | RenameFlags::RENAME_WHITEOUT;
        if !(flags & !known).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        if flags.contains(RenameFlags::RENAME_EXCHANGE)
            && flags.intersects(RenameFlags::RENAME_NOREPLACE | RenameFlags::RENAME_WHITEOUT)
        {
            return Err(Errno::EINVAL.into());
        }
        let mode = if flags.contains(RenameFlags::RENAME_EXCHANGE) {
            RenameMode::Exchange
        } else if flags.contains(RenameFlags::RENAME_WHITEOUT) {
            RenameMode::Whiteout
        } else if flags.contains(RenameFlags::RENAME_NOREPLACE) {
            RenameMode::NoReplace
        } else {
            RenameMode::Replace
        };
        let old = read_path(caller.memory(), oldname).await?;
        let new = read_path(caller.memory(), newname).await?;
        let env = self.env(caller);
        let old_start = self.start(caller, &env, olddfd, &old)?;
        let new_start = self.start(caller, &env, newdfd, &new)?;
        self.vfs
            .rename(&env.cx(), &old_start, &old, &new_start, &new, mode)
            .await?;
        Ok(0)
    }

    /// `readlinkat`; an empty path reads the symlink `dfd` refers to (an `O_PATH` descriptor).
    pub(crate) async fn sys_readlinkat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        pathname: Cstr,
        buf: Buf,
        bufsiz: i32,
    ) -> SysResult<i32> {
        if bufsiz <= 0 {
            return Err(Errno::EINVAL.into());
        }
        let path = read_path(caller.memory(), pathname).await?;
        let env = self.env(caller);
        let start = if path.is_empty() {
            self.start_or_fd(caller, &env, dfd)?
        } else {
            self.start(caller, &env, dfd, &path)?
        };
        let target = self
            .vfs
            .readlink(&env.cx(), &start, &path, path.is_empty())
            .await
            .map_err(|errno| match errno {
                // An empty path that does not name a symlink.
                Errno::EINVAL if path.is_empty() => Errno::ENOENT,
                errno => errno,
            })?;
        let n = target.len().min(bufsiz as usize);
        UserSliceMut::new(buf.addr(), n as u32)
            .copy_out(caller.memory(), &target[..n])
            .await?;
        Ok(n as i32)
    }

    pub(crate) async fn sys_chdir(&self, caller: &Caller<'_>, filename: Cstr) -> SysResult<i32> {
        let path = read_path(caller.memory(), filename).await?;
        let env = self.env(caller);
        let start = env.fs.cwd.clone();
        let lookup = LookupFlags {
            follow: true,
            directory: true,
            ..LookupFlags::default()
        };
        let dir = self.vfs.lookup(&env.cx(), &start, &path, lookup).await?;
        self.vfs.permission(&env.cred, &dir, MAY_EXEC).await?;
        self.with_task(caller.task(), |task| task.fs.lock().unwrap().cwd = dir);
        Ok(0)
    }

    pub(crate) async fn sys_fchdir(&self, caller: &Caller<'_>, fd: Fd) -> SysResult<i32> {
        let dir = self.fd_path(caller, fd)?;
        if !dir.dentry.is_dir() {
            return Err(Errno::ENOTDIR.into());
        }
        let env = self.env(caller);
        self.vfs.permission(&env.cred, &dir, MAY_EXEC).await?;
        self.with_task(caller.task(), |task| task.fs.lock().unwrap().cwd = dir);
        Ok(0)
    }

    /// `chroot`: needs `CAP_SYS_CHROOT`.
    pub(crate) async fn sys_chroot(&self, caller: &Caller<'_>, filename: Cstr) -> SysResult<i32> {
        let path = read_path(caller.memory(), filename).await?;
        let env = self.env(caller);
        let lookup = LookupFlags {
            follow: true,
            directory: true,
            ..LookupFlags::default()
        };
        let dir = self
            .vfs
            .lookup(&env.cx(), &env.fs.cwd, &path, lookup)
            .await?;
        self.vfs.permission(&env.cred, &dir, MAY_EXEC).await?;
        if !env.cred.capable(Cap::SysChroot) {
            return Err(Errno::EPERM.into());
        }
        self.with_task(caller.task(), |task| task.fs.lock().unwrap().root = dir);
        Ok(0)
    }

    /// `getcwd`: the length including the NUL.
    pub(crate) async fn sys_getcwd(
        &self,
        caller: &Caller<'_>,
        buf: UserSliceMut,
    ) -> SysResult<u32> {
        let env = self.env(caller);
        if env.fs.cwd.dentry.is_unlinked() {
            return Err(Errno::ENOENT.into());
        }
        let mut path = self.vfs.d_path(&env.fs.cwd, &env.fs.root);
        path.push(0);
        if path.len() > buf.len as usize {
            return Err(Errno::ERANGE.into());
        }
        buf.copy_out(caller.memory(), &path).await?;
        Ok(path.len() as u32)
    }

    pub(crate) fn sys_umask(&self, caller: &Caller<'_>, mask: i32) -> SysResult<i32> {
        let old = self.with_task(caller.task(), |task| {
            let mut fs = task.fs.lock().unwrap();
            std::mem::replace(&mut fs.umask, mask as u32 & 0o777)
        });
        Ok(old as i32)
    }

    pub(crate) async fn sys_truncate(
        &self,
        caller: &Caller<'_>,
        pathname: Cstr,
        length: i64,
    ) -> SysResult<i32> {
        if length < 0 {
            return Err(Errno::EINVAL.into());
        }
        let path = read_path(caller.memory(), pathname).await?;
        let env = self.env(caller);
        let start = env.fs.cwd.clone();
        let target = self
            .vfs
            .lookup(&env.cx(), &start, &path, LookupFlags::follow())
            .await?;
        self.vfs.truncate(&env.cx(), &target, length as u64).await?;
        Ok(0)
    }
}
