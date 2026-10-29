//! `kcmp` (kernel/kcmp.c): whether two tasks share a kernel resource, for checkpointing tools
//! such as CRIU. Resources are compared by identity, in an order that hides their addresses:
//! 0 for the same, 1 or 2 for an order between different ones (`kptr_obfuscate`), stable for
//! the kernel's life.

use std::sync::Arc;

use wasixv2_abi::{Errno, GuestPtr, KcmpEpollSlot, KcmpType, Pid, Ulong};
use wasixv2_syscalls::{Caller, SysResult, TaskId, read_value};

use crate::{Kernel, epoll::Epoll, ptrace::MODE_READ_REALCREDS, sync::MutexExt as _};

/// `KCMP_TYPES`.
const TYPES: usize = 8;

/// `cookies`: per type, a value to mix in and an odd multiplier with its top bit set.
#[derive(Debug)]
pub(crate) struct Cookies([[u64; 2]; TYPES]);

impl Cookies {
    fn new(host: &dyn wasixv2_host::Host) -> Self {
        let mut bytes = [0; TYPES * 16];
        host.random(&mut bytes);
        let mut cookies = [[0; 2]; TYPES];
        for (i, cookie) in cookies.iter_mut().enumerate() {
            let word = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
            *cookie = [word(i * 16), word(i * 16 + 8) | (1 << 63) | 1];
        }
        Self(cookies)
    }

    /// `kptr_obfuscate`.
    fn obfuscate(&self, v: usize, ty: usize) -> i64 {
        let [mix, mul] = self.0[ty];
        ((v as u64 ^ mix).wrapping_mul(mul)) as i64
    }

    /// `kcmp_ptr`: 0 for equal, 1 for less, 2 for greater.
    fn cmp(&self, v1: usize, v2: usize, ty: KcmpType) -> i32 {
        let ty = ty.0 as usize;
        let (t1, t2) = (self.obfuscate(v1, ty), self.obfuscate(v2, ty));
        i32::from(t1 < t2) | (i32::from(t1 > t2) << 1)
    }
}

/// The address of what an `Arc` holds: its identity.
fn id<T: ?Sized>(arc: &Arc<T>) -> usize {
    Arc::as_ptr(arc).cast::<()>() as usize
}

impl Kernel {
    fn kcmp_cookies(&self) -> &Cookies {
        self.kcmp.get_or_init(|| Cookies::new(&*self.host))
    }

    /// The identity of `task`'s resource of type `ty` (everything but files), 0 for none, as
    /// Linux's `NULL` pointers compare equal.
    fn kcmp_resource(&self, task: TaskId, ty: KcmpType) -> Result<usize, Errno> {
        let state = self.state.read_or_recover();
        let t = state.tasks.get(&task).ok_or(Errno::ESRCH)?;
        Ok(match ty {
            KcmpType::KCMP_VM => id(&t.mm),
            KcmpType::KCMP_FILES => id(&t.files),
            KcmpType::KCMP_FS => id(&t.fs),
            KcmpType::KCMP_SIGHAND => t.signals.group().sighand_id(),
            KcmpType::KCMP_IO => t.io.as_ref().map_or(0, id),
            KcmpType::KCMP_SYSVSEM => {
                let process = &t.process;
                let ipc = process.sysvsem.lock_or_recover().clone();
                match ipc.is_some_and(|ipc| ipc.sem.has_undo(process.pid.0)) {
                    true => id(process),
                    false => 0,
                }
            }
            _ => return Err(Errno::EINVAL),
        })
    }

    /// `get_file_raw_ptr`: the open file at `fd` in `task`'s table.
    fn kcmp_file(&self, task: TaskId, fd: Ulong) -> Result<Arc<crate::file::OpenFile>, Errno> {
        let files = {
            let state = self.state.read_or_recover();
            state.tasks.get(&task).map(|t| t.files.clone())
        };
        let fd = i32::try_from(fd).map_err(|_| Errno::EBADF)?;
        let file = files.and_then(|files| files.lock_or_recover().get(wasixv2_abi::Fd(fd)));
        file.ok_or(Errno::EBADF)
    }

    /// `kcmp`: the tasks `pid1` and `pid2` in the caller's pid namespace, which the caller may
    /// read (`PTRACE_MODE_READ_REALCREDS`, else `EPERM`), compared on `ty`.
    pub(crate) async fn sys_kcmp(
        &self,
        caller: &Caller<'_>,
        pid1: Pid,
        pid2: Pid,
        ty: i32,
        idx1: Ulong,
        idx2: Ulong,
    ) -> SysResult<i32> {
        // `find_task_by_vpid`: 0 is no task.
        let find = |pid: Pid| {
            (pid > 0)
                .then(|| self.find_vpid(caller, pid))
                .flatten()
                .map(TaskId)
                .filter(|&task| self.task_process(task).is_some())
        };
        let (Some(t1), Some(t2)) = (find(pid1), find(pid2)) else {
            return Err(Errno::ESRCH.into());
        };
        let me = caller.task();
        if !self.ptrace_may_access(me, t1, MODE_READ_REALCREDS)
            || !self.ptrace_may_access(me, t2, MODE_READ_REALCREDS)
        {
            return Err(Errno::EPERM.into());
        }
        let ty = KcmpType(ty);
        let cookies = self.kcmp_cookies();
        let (v1, v2) = match ty {
            KcmpType::KCMP_FILE => {
                let f1 = self.kcmp_file(t1, idx1);
                let f2 = self.kcmp_file(t2, idx2);
                (id(&f1?), id(&f2?))
            }
            KcmpType::KCMP_EPOLL_TFD => {
                let slot: KcmpEpollSlot = read_value(caller.memory(), GuestPtr::new(idx2)).await?;
                let file = self.kcmp_file(t1, idx1)?;
                let epoll = self.kcmp_file(t2, Ulong::from(slot.efd))?;
                let epoll = epoll.downcast::<Epoll>().ok_or(Errno::EINVAL)?;
                let tfd = slot.tfd as i32;
                let target = epoll.target_file(tfd, slot.toff).ok_or(Errno::ENOENT)?;
                (id(&file), id(&target))
            }
            ty => (self.kcmp_resource(t1, ty)?, self.kcmp_resource(t2, ty)?),
        };
        let ty = match ty {
            KcmpType::KCMP_EPOLL_TFD => KcmpType::KCMP_FILE,
            ty => ty,
        };
        Ok(cookies.cmp(v1, v2, ty))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_is_0_and_different_is_an_order() {
        let cookies = Cookies([[0x1234, 0x8000_0000_0000_0001]; TYPES]);
        assert_eq!(cookies.cmp(10, 10, KcmpType::KCMP_VM), 0);
        let (a, b) = (
            cookies.cmp(10, 20, KcmpType::KCMP_VM),
            cookies.cmp(20, 10, KcmpType::KCMP_VM),
        );
        assert!(a == 1 && b == 2 || a == 2 && b == 1);
    }
}
