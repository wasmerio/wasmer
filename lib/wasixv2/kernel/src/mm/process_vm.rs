//! `process_vm_readv` and `process_vm_writev` (mm/process_vm_access.c): copies between the
//! caller's memory and another process's, which the caller may attach to, through the memory
//! the execution layer handed the kernel ([`access_remote_vm`]). The pages' protection counts
//! (no `FOLL_FORCE`).

use wasixv2_abi::{ConstIovecArray, Iovec, Pid, Ulong};
use wasixv2_syscalls::{Caller, Errno, SysResult, TaskId};

use crate::{
    Kernel,
    ptrace::{Access, MODE_ATTACH_REALCREDS, access_remote_vm},
};

/// How much one step moves through kernel memory.
const CHUNK: usize = 64 * 1024;

impl Kernel {
    /// `process_vm_rw`: the local iovecs `lvec` and the remote ones `rvec` of the process `pid`
    /// are walked together; returns how many bytes moved, or the error that kept the first one
    /// from moving (`EFAULT` for a remote range that faults, `EPERM` for a process the caller
    /// may not attach to, `ESRCH` for none).
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn sys_process_vm_rw(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        lvec: ConstIovecArray,
        liovcnt: Ulong,
        rvec: ConstIovecArray,
        riovcnt: Ulong,
        flags: Ulong,
        write: bool,
    ) -> SysResult<i32> {
        if flags != 0 {
            return Err(Errno::EINVAL.into());
        }
        let memory = caller.memory();
        // mm/process_vm_access.c:process_vm_rw imports the remote list only
        // after a nonempty local iterator; process_vm_rw_core looks up no task
        // when every remote vector is empty.
        if liovcnt == 0 {
            return Ok(0);
        }
        let local = crate::fs::read_iovecs(memory, lvec, liovcnt).await?;
        if local.iter().all(|iov| iov.iov_len == 0) {
            return Ok(0);
        }
        if riovcnt == 0 {
            return Ok(0);
        }
        let remote = crate::fs::read_iovecs(memory, rvec, riovcnt).await?;
        if remote.iter().all(|iov| iov.iov_len == 0) {
            return Ok(0);
        }
        let task = self
            .find_vpid(caller, pid)
            .map(TaskId)
            .filter(|&task| pid > 0 && self.task_process(task).is_some())
            .ok_or(Errno::ESRCH)?;
        let mm = match self.mm_access(caller.task(), task, MODE_ATTACH_REALCREDS) {
            Ok(mm) => mm,
            Err(Errno::EACCES) => return Err(Errno::EPERM.into()),
            Err(errno) => return Err(errno.into()),
        };
        let (mut l, mut r) = (Cursor::new(&local), Cursor::new(&remote));
        let mut done = 0u32;
        let mut buf = vec![0; CHUNK];
        let error = loop {
            let (Some(at), Some(there)) = (l.next(), r.next()) else {
                break None;
            };
            // process_vm_rw_pages copies through the local iterator a page at
            // a time: a fault on the next page keeps the successful prefix.
            let local_page = super::PAGE_SIZE - u64::from(at.0) % super::PAGE_SIZE;
            let n = (at.1.min(there.1) as usize)
                .min(CHUNK)
                .min(local_page as usize);
            let buf = &mut buf[..n];
            let moved = match write {
                false => {
                    let read = access_remote_vm(&mm, there.0, Access::Read(buf), false).await;
                    if read > 0 && memory.write(at.0, &buf[..read]).await.is_err() {
                        break Some(Errno::EFAULT);
                    }
                    read
                }
                true => {
                    if memory.read(at.0, buf).await.is_err() {
                        break Some(Errno::EFAULT);
                    }
                    access_remote_vm(&mm, there.0, Access::Write(buf), false).await
                }
            };
            done += moved as u32;
            if moved < n {
                break Some(Errno::EFAULT);
            }
            l.advance(n as u32);
            r.advance(n as u32);
        };
        match (done, error) {
            (0, Some(errno)) => Err(errno.into()),
            (done, _) => Ok(done as i32),
        }
    }
}

/// A position in a list of iovecs.
struct Cursor<'a> {
    iovecs: &'a [Iovec],
    index: usize,
    offset: u32,
}

impl<'a> Cursor<'a> {
    fn new(iovecs: &'a [Iovec]) -> Self {
        Self {
            iovecs,
            index: 0,
            offset: 0,
        }
    }

    /// The address and length left of the current iovec, past empty ones.
    fn next(&mut self) -> Option<(u32, u32)> {
        loop {
            let iov = self.iovecs.get(self.index)?;
            if self.offset < iov.iov_len {
                return iov
                    .iov_base
                    .checked_add(self.offset)
                    .map(|addr| (addr, iov.iov_len - self.offset));
            }
            (self.index, self.offset) = (self.index + 1, 0);
        }
    }

    fn advance(&mut self, n: u32) {
        self.offset += n;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_iovec_stops_at_the_guest_address_limit() {
        let vectors = [Iovec {
            iov_base: u32::MAX - 3,
            iov_len: 8,
        }];
        let mut cursor = Cursor::new(&vectors);
        assert_eq!(cursor.next(), Some((u32::MAX - 3, 8)));
        cursor.advance(4);
        assert_eq!(cursor.next(), None);
    }
}
