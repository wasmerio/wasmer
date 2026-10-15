//! `flock` and the `fcntl` record-lock commands (`fs/locks.c`).

use wasixv2_abi::{FcntlCmd, Flock, Flock64, FlockOp, GuestLayout, LockType, SeekWhence};
use wasixv2_syscalls::{Caller, Errno, Fd, SysResult};
use wasixv2_vfs::{FlockOp as Op, LockKind, LockOwner, RangeLock};

use crate::{
    Kernel,
    file::{Backing, OpenFile},
};

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
        // TODO: streams (pipes, sockets) can be flocked on Linux too; they have no lock key yet.
        let Backing::Inode(inner) = &file.backing else {
            return Ok(0);
        };
        self.vfs
            .locks
            .flock(inner.lock_key(), inner.id, op, nonblock)
            .await?;
        Ok(0)
    }

    /// `F_GETLK`, `F_SETLK`, `F_SETLKW`, their `64` and `F_OFD_` forms.
    pub(crate) async fn fcntl_lock(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
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
                    l_pid: match lock.owner {
                        LockOwner::File(_) => -1,
                        LockOwner::Process(_) => lock.pid,
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
                self.vfs.locks.set(key, lock, wait).await?;
            }
        }
        Ok(0)
    }
}

/// `flock_to_posix_lock`: the inclusive byte range a request covers.
fn range(base: i64, l_start: i64, l_len: i64) -> Result<(u64, u64), Errno> {
    let mut start = base.checked_add(l_start).ok_or(Errno::EOVERFLOW)?;
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
