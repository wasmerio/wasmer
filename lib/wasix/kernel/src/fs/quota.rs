//! `quotactl` / `quotactl_fd`, fs/quota/quota.c. Tmpfs has no block device, so it is
//! addressed through a descriptor; filesystem drivers own accounting and enforcement.

use wasixv2_abi::{
    Cstr, GuestPtr, GuestPtrMut, IfDqblk, IfDqinfo, IfNextdqblk, Qid, QuotaMisc as Q, VoidPtr,
};
use wasixv2_syscalls::{Caller, Errno, Fd, SysResult, read_value, write_value};
use wasixv2_vfs::{Cap, FileType, LookupFlags, UserNs};

use super::read_path;
use crate::Kernel;

fn command(cmd: u32) -> Result<(u32, usize), Errno> {
    let kind = cmd & Q::SUBCMDMASK.0;
    if kind >= 3 {
        return Err(Errno::EINVAL);
    }
    Ok((cmd >> Q::SUBCMDSHIFT.0, kind as usize))
}

impl Kernel {
    pub(crate) async fn sys_quotactl(
        &self,
        caller: &Caller<'_>,
        cmd: u32,
        special: Cstr,
        _id: Qid,
        _addr: VoidPtr,
    ) -> SysResult<i32> {
        let (cmd, _) = command(cmd)?;
        if special.is_null() {
            return if cmd == Q::Q_SYNC.0 {
                Ok(0)
            } else {
                Err(Errno::ENODEV.into())
            };
        }
        let name = read_path(caller.memory(), special).await?;
        let env = self.env(caller);
        let path = self
            .vfs
            .lookup(&env.cx(), &env.fs.cwd, &name, LookupFlags::follow())
            .await?;
        // quotactl_block -> lookup_bdev: this kernel has no mounted block filesystems.
        if path.dentry.node().file_type() != FileType::BlockDevice {
            return Err(Errno::ENOTBLK.into());
        }
        Err(Errno::ENODEV.into())
    }

    pub(crate) async fn sys_quotactl_fd(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        cmd: u32,
        id: Qid,
        addr: VoidPtr,
    ) -> SysResult<i32> {
        // fd_raw precedes even type validation, and accepts O_PATH descriptors.
        let path = self.fd_path(caller, fd)?;
        let (cmd, kind) = command(cmd)?;
        let writes = [
            Q::Q_QUOTAON.0,
            Q::Q_QUOTAOFF.0,
            Q::Q_SETINFO.0,
            Q::Q_SETQUOTA.0,
        ]
        .contains(&cmd);
        if writes && path.mnt.is_read_only() {
            return Err(Errno::EROFS.into());
        }
        let quota = path.dentry.sb().fs.quota().ok_or(Errno::ENOSYS)?;
        if kind >= 2 {
            return Err(Errno::EINVAL.into());
        }
        // check_quotactl_permission uses effective ids and the initial namespace's capability.
        let cred = self.cred(caller);
        let own = match kind {
            0 => cred
                .user_ns
                .make_kuid(id)
                .is_some_and(|uid| uid == cred.user.effective),
            _ => cred
                .user_ns
                .make_kgid(id)
                .is_some_and(|gid| gid == cred.group.effective || cred.groups.contains(&gid)),
        };
        if !([Q::Q_GETFMT.0, Q::Q_SYNC.0, Q::Q_GETINFO.0].contains(&cmd)
            || cmd == Q::Q_GETQUOTA.0 && own
            || cred.capable_init(Cap::SysAdmin))
        {
            return Err(Errno::EPERM.into());
        }
        let memory = caller.memory();
        match cmd {
            op if op == Q::Q_SYNC.0 => {}
            op if op == Q::Q_QUOTAON.0 => quota.enforce(kind, true)?,
            op if op == Q::Q_QUOTAOFF.0 => quota.enforce(kind, false)?,
            op if op == Q::Q_GETFMT.0 => {
                if !quota.active(kind) {
                    return Err(Errno::ESRCH.into());
                }
                write_value(memory, GuestPtrMut::new(addr.addr()), &Q::QFMT_SHMEM.0).await?;
            }
            op if op == Q::Q_GETINFO.0 => {
                write_value(memory, GuestPtrMut::new(addr.addr()), &quota.info(kind)?).await?;
            }
            op if op == Q::Q_SETINFO.0 => {
                let info: IfDqinfo = read_value(memory, GuestPtr::new(addr.addr())).await?;
                quota.set_info(kind, &info)?;
            }
            op if op == Q::Q_GETQUOTA.0 || op == Q::Q_SETQUOTA.0 || op == Q::Q_GETNEXTQUOTA.0 => {
                // quota_setquota copies its argument before mapping the supplied id.
                let limits: Option<IfDqblk> = if op == Q::Q_SETQUOTA.0 {
                    Some(read_value(memory, GuestPtr::new(addr.addr())).await?)
                } else {
                    None
                };
                let mapped = match kind {
                    0 => cred.user_ns.make_kuid(id).map(|id| id.0),
                    _ => cred.user_ns.make_kgid(id).map(|id| id.0),
                }
                .ok_or(Errno::EINVAL)?;
                if let Some(limits) = limits {
                    quota.set(kind, mapped, &limits)?;
                } else if op == Q::Q_GETQUOTA.0 {
                    write_value(
                        memory,
                        GuestPtrMut::new(addr.addr()),
                        &quota.get(kind, mapped)?,
                    )
                    .await?;
                } else {
                    let (id, dq) = quota.next(kind, mapped)?;
                    let id = match kind {
                        0 => cred.user_ns.ns_uid(id),
                        _ => cred.user_ns.ns_gid(id),
                    }
                    .unwrap_or(65534);
                    let out = IfNextdqblk {
                        dqb_bhardlimit: dq.dqb_bhardlimit,
                        dqb_bsoftlimit: dq.dqb_bsoftlimit,
                        dqb_curspace: dq.dqb_curspace,
                        dqb_ihardlimit: dq.dqb_ihardlimit,
                        dqb_isoftlimit: dq.dqb_isoftlimit,
                        dqb_curinodes: dq.dqb_curinodes,
                        dqb_btime: dq.dqb_btime,
                        dqb_itime: dq.dqb_itime,
                        dqb_valid: dq.dqb_valid,
                        dqb_id: id,
                    };
                    write_value(memory, GuestPtrMut::new(addr.addr()), &out).await?;
                }
            }
            _ => return Err(Errno::ENOSYS.into()),
        }
        Ok(0)
    }
}
