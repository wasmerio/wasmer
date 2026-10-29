//! `flock`, the `fcntl` record-lock commands and leases (`fs/locks.c`).

use std::sync::{Arc, Weak};

use wasixv2_abi::{FcntlCmd, Flock, Flock64, FlockOp, GuestLayout, LockType, SeekWhence, SiCode};
use wasixv2_syscalls::{Caller, Errno, Fd, SysErr, SysResult};
use wasixv2_vfs::{Cap, FileType, FlockOp as Op, LeaseHolder, LockKind, LockOwner, RangeLock};
use wasixv2_wait::{Interrupted, interruptible};

use crate::{
    Kernel,
    fasync::{OwnerType, Sigio},
    file::{Backing, OpenFile},
    sync::MutexExt,
};

/// What holds a lease (`lease_manager_ops`): a break sends the file's owner `SIGIO`, or the
/// `F_SETSIG` signal with `POLL_MSG` and the descriptor `F_SETLEASE` came through
/// (`lease_break_callback`'s `kill_fasync`).
struct Holder {
    file: Weak<OpenFile>,
    fd: Fd,
    sigio: Arc<Sigio>,
}

impl LeaseHolder for Holder {
    fn lease_break(&self) {
        if let Some(file) = self.file.upgrade() {
            self.sigio.send(&file, self.fd, SiCode::POLL_MSG);
        }
    }

    fn lease_removed(&self) {
        if let Some(file) = self.file.upgrade() {
            file.delown();
        }
    }
}

/// The POSIX lock owner of the calling process.
pub(crate) fn process_owner(kernel: &Kernel, caller: &Caller<'_>) -> LockOwner {
    LockOwner::Process(u64::from(kernel.process(caller.task()).pid.0))
}

/// A lock request in `flock64` terms, from either struct layout.
struct Request {
    l_type: LockType,
    l_whence: i16,
    l_start: i64,
    l_len: i64,
    l_pid: i32,
}

impl Kernel {
    pub(crate) async fn sys_flock(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        cmd: FlockOp,
    ) -> SysResult<i32> {
        let file = self.io_file(caller, fd)?;
        let nonblock = cmd.contains(FlockOp::LOCK_NB);
        let op = match FlockOp(cmd.0 & !FlockOp::LOCK_NB.0) {
            FlockOp::LOCK_SH => Op::Shared,
            FlockOp::LOCK_EX => Op::Exclusive,
            FlockOp::LOCK_UN => Op::Unlock,
            op if op.contains(FlockOp::LOCK_MAND) => return Err(Errno::EOPNOTSUPP.into()),
            _ => return Err(Errno::EINVAL.into()),
        };
        // Pipes and sockets are locked through their pseudo inodes, as any file is.
        let (key, owner) = file.flock_owner(&self.vfs);
        let lock = self.vfs.locks.flock(key, owner, op, nonblock);
        self.lock_wait(caller, lock).await?;
        Ok(0)
    }

    /// `F_GETLK`, `F_SETLK`, `F_SETLKW`, their `64` and `F_OFD_` forms.
    pub(crate) async fn fcntl_lock(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        file: &Arc<OpenFile>,
        cmd: FcntlCmd,
        arg: u32,
    ) -> SysResult<i32> {
        let (legacy, ofd, get, wait) = match cmd {
            FcntlCmd::F_GETLK => (true, false, true, false),
            FcntlCmd::F_SETLK => (true, false, false, false),
            FcntlCmd::F_SETLKW => (true, false, false, true),
            FcntlCmd::F_GETLK64 => (false, false, true, false),
            FcntlCmd::F_SETLK64 => (false, false, false, false),
            FcntlCmd::F_SETLKW64 => (false, false, false, true),
            FcntlCmd::F_OFD_GETLK => (false, true, true, false),
            FcntlCmd::F_OFD_SETLK => (false, true, false, false),
            FcntlCmd::F_OFD_SETLKW => (false, true, false, true),
            _ => return Err(Errno::EINVAL.into()),
        };
        let request = read_request(caller, arg, legacy).await?;
        let Backing::Inode(inner) = &file.backing else {
            return Err(Errno::EBADF.into());
        };
        if ofd && request.l_pid != 0 {
            return Err(Errno::EINVAL.into());
        }
        let base = match SeekWhence(i32::from(request.l_whence)) {
            SeekWhence::SEEK_SET => 0,
            SeekWhence::SEEK_CUR => file.pos() as i64,
            SeekWhence::SEEK_END => inner.node().getattr().await?.size as i64,
            _ => return Err(Errno::EINVAL.into()),
        };
        let (start, end) = range(base, request.l_start, request.l_len)?;
        let owner = if ofd {
            LockOwner::File(inner.id)
        } else {
            process_owner(self, caller)
        };
        let pid = self.process(caller.task()).pid.0 as i32;
        let kind = match request.l_type {
            LockType::F_RDLCK => Some(LockKind::Read),
            LockType::F_WRLCK => Some(LockKind::Write),
            LockType::F_UNLCK => None,
            _ => return Err(Errno::EINVAL.into()),
        };
        let key = inner.lock_key();
        if get {
            let kind = kind.ok_or(Errno::EINVAL)?;
            let probe = RangeLock {
                owner,
                pid,
                kind,
                start,
                end,
            };
            let reply = match self.vfs.locks.test(key, &probe) {
                None => Request {
                    l_type: LockType::F_UNLCK,
                    ..request
                },
                Some(lock) => Request {
                    l_type: match lock.kind {
                        LockKind::Read => LockType::F_RDLCK,
                        LockKind::Write => LockType::F_WRLCK,
                    },
                    l_whence: SeekWhence::SEEK_SET.0 as i16,
                    l_start: lock.start as i64,
                    l_len: if lock.end == u64::MAX {
                        0
                    } else {
                        (lock.end - lock.start + 1) as i64
                    },
                    // `locks_translate_pid`: the holder as the caller sees it.
                    l_pid: match lock.owner {
                        LockOwner::File(_) => -1,
                        LockOwner::Process(_) => self.vnr(caller, lock.pid as u32),
                    },
                },
            };
            write_request(caller, arg, legacy, &reply).await?;
            return Ok(0);
        }
        match kind {
            None => self.vfs.locks.unlock(key, owner, start, end),
            Some(kind) => {
                // Read locks need a readable file, write locks a writable one.
                let allowed = match kind {
                    LockKind::Read => file.check_readable(),
                    LockKind::Write => file.check_writable(),
                };
                allowed?;
                let lock = RangeLock {
                    owner,
                    pid,
                    kind,
                    start,
                    end,
                };
                self.lock_wait(caller, self.vfs.locks.set(key, lock, wait))
                    .await?;
                // `fcntl_setlk`: a close of `fd` that raced with the wait released this
                // process's locks before this one was set, so release it too.
                if !ofd && !self.still_open(caller, fd, file) {
                    self.vfs.locks.release_owner(key, owner);
                    return Err(Errno::EBADF.into());
                }
            }
        }
        Ok(0)
    }
}

impl Kernel {
    /// `fcntl_setlease`: `F_SETLEASE` on `file`, open as `fd`, with `F_RDLCK`, `F_WRLCK` or
    /// `F_UNLCK` (`vfs_setlease`, `generic_setlease`). Only a regular file's owner, or a
    /// caller with `CAP_LEASE`, takes leases (`EACCES`); a new lease makes the caller's process
    /// the file's owner unless it has one (`lease_setup`).
    pub(crate) async fn fcntl_setlease(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        file: &Arc<OpenFile>,
        arg: u32,
    ) -> SysResult<i32> {
        let Backing::Inode(inner) = &file.backing else {
            return Err(Errno::EINVAL.into());
        };
        if inner.file_type() == FileType::Directory {
            return Err(Errno::EINVAL.into());
        }
        let kind = match i16::try_from(arg).map(LockType) {
            Ok(LockType::F_RDLCK) => Some(LockKind::Read),
            Ok(LockType::F_WRLCK) => Some(LockKind::Write),
            Ok(LockType::F_UNLCK) => None,
            _ => return Err(Errno::EINVAL.into()),
        };
        let cred = self.env(caller).cred;
        let attr = inner.node().getattr().await?;
        if cred.fsuid != attr.uid && !cred.capable(Cap::Lease) {
            return Err(Errno::EACCES.into());
        }
        // `generic_setlease` takes regular files only, and `kernel_setlease` only on a
        // filesystem with `->setlease`, which procfs is not.
        let proc = inner.path.dentry.sb().info.magic == crate::procfs::PROC_SUPER_MAGIC;
        if inner.file_type() != FileType::Regular || proc {
            return Err(Errno::EINVAL.into());
        }
        let now = self.host.monotonic();
        let Some(kind) = kind else {
            inner.remove_lease(now)?;
            return Ok(0);
        };
        let holder = Arc::new(Holder {
            file: Arc::downgrade(file),
            fd,
            sigio: self.sigio.clone(),
        });
        let enabled = self.sysctls.leases_enable() != 0;
        inner.set_lease(kind, holder, enabled, now)?;
        let pid = self.process(caller.task()).pid;
        let creds = self.cred(caller);
        let uids = (creds.user.real, creds.user.effective);
        file.set_owner(Some(pid.0), OwnerType::Process, uids, false);
        Ok(0)
    }

    /// `fcntl_getlease`: the lease `file` holds, as it is to become while it is being broken.
    pub(crate) fn fcntl_getlease(&self, file: &OpenFile) -> i32 {
        let lease = match &file.backing {
            Backing::Inode(inner) => inner.lease(self.host.monotonic()),
            Backing::Stream(..) => None,
        };
        let kind = match lease {
            Some(LockKind::Read) => LockType::F_RDLCK,
            Some(LockKind::Write) => LockType::F_WRLCK,
            None => LockType::F_UNLCK,
        };
        i32::from(kind.0)
    }

    /// Whether `fd` still refers to `file`.
    fn still_open(&self, caller: &Caller<'_>, fd: Fd, file: &Arc<OpenFile>) -> bool {
        let files = self.files(caller.task());
        let files = files.lock_or_recover();
        files.get(fd).is_some_and(|open| Arc::ptr_eq(&open, file))
    }

    /// A lock request that may wait for a conflicting lock to go. A signal ends the wait with
    /// a restart, as `wait_event_interruptible` does in `flock_lock_inode_wait` and
    /// `fcntl_setlk`; the lock manager forgets the request when it is dropped.
    async fn lock_wait(
        &self,
        caller: &Caller<'_>,
        request: impl Future<Output = Result<(), Errno>>,
    ) -> SysResult<()> {
        match interruptible(request, &*self.interrupt(caller)).await {
            Ok(result) => Ok(result?),
            Err(Interrupted) => Err(SysErr::Restart),
        }
    }
}

/// `flock_to_posix_lock`: the inclusive byte range a request covers.
fn range(base: i64, l_start: i64, l_len: i64) -> Result<(u64, u64), Errno> {
    let mut start = base.checked_add(l_start).ok_or(Errno::EOVERFLOW)?;
    if start < 0 {
        return Err(Errno::EINVAL);
    }
    let end = if l_len > 0 {
        start.checked_add(l_len - 1).ok_or(Errno::EOVERFLOW)?
    } else if l_len < 0 {
        let end = start - 1;
        start = start.checked_add(l_len).ok_or(Errno::EINVAL)?;
        end
    } else {
        i64::MAX
    };
    if start < 0 {
        return Err(Errno::EINVAL);
    }
    let end = if end == i64::MAX {
        u64::MAX
    } else {
        end as u64
    };
    Ok((start as u64, end))
}

async fn read_request(caller: &Caller<'_>, addr: u32, legacy: bool) -> Result<Request, Errno> {
    let size = if legacy { Flock::SIZE } else { Flock64::SIZE };
    let mut raw = vec![0; size];
    caller
        .memory()
        .read(addr, &mut raw)
        .await
        .map_err(|_| Errno::EFAULT)?;
    Ok(if legacy {
        let lock = Flock::read_from(&raw).ok_or(Errno::EFAULT)?;
        Request {
            l_type: lock.l_type,
            l_whence: lock.l_whence,
            l_start: i64::from(lock.l_start),
            l_len: i64::from(lock.l_len),
            l_pid: lock.l_pid,
        }
    } else {
        let lock = Flock64::read_from(&raw).ok_or(Errno::EFAULT)?;
        Request {
            l_type: lock.l_type,
            l_whence: lock.l_whence,
            l_start: lock.l_start,
            l_len: lock.l_len,
            l_pid: lock.l_pid,
        }
    })
}

async fn write_request(
    caller: &Caller<'_>,
    addr: u32,
    legacy: bool,
    reply: &Request,
) -> Result<(), Errno> {
    let raw = if legacy {
        let narrow = |v: i64| i32::try_from(v).map_err(|_| Errno::EOVERFLOW);
        let lock = Flock {
            l_type: reply.l_type,
            l_whence: reply.l_whence,
            l_start: narrow(reply.l_start)?,
            l_len: narrow(reply.l_len)?,
            l_pid: reply.l_pid,
        };
        let mut raw = vec![0; Flock::SIZE];
        lock.write_to(&mut raw);
        raw
    } else {
        let lock = Flock64 {
            l_type: reply.l_type,
            l_whence: reply.l_whence,
            l_start: reply.l_start,
            l_len: reply.l_len,
            l_pid: reply.l_pid,
        };
        let mut raw = vec![0; Flock64::SIZE];
        lock.write_to(&mut raw);
        raw
    };
    caller
        .memory()
        .write(addr, &raw)
        .await
        .map_err(|_| Errno::EFAULT)
}
