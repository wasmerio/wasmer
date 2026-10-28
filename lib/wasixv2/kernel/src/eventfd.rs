//! Event counters, as Linux's `fs/eventfd.c`.

use std::sync::{Arc, Mutex};

use wasixv2_abi::{EventfdFlags, OpenFlags};
use wasixv2_syscalls::{Caller, Errno, Fd, LocalFuture, SysErr, SysResult};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait, wait_event};

use crate::{
    Kernel,
    file::{Backing, FileObject, Io, OpenFile},
    sync::MutexExt,
};

const MAX: u64 = u64::MAX;

pub(crate) struct EventFd {
    count: Mutex<u64>,
    semaphore: bool,
    wait: WaitQueue,
}

impl EventFd {
    /// `eventfd_read`.
    async fn read_count(&self, io: Io<'_>, buf: &mut [u8]) -> SysResult<usize> {
        if buf.len() < 8 {
            return Err(Errno::EINVAL.into());
        }
        let take = || {
            let mut count = self.count.lock_or_recover();
            if *count == 0 {
                return io.nonblocking().then_some(Err(Errno::EAGAIN));
            }
            let value = if self.semaphore { 1 } else { *count };
            *count -= value;
            Some(Ok(value))
        };
        let value = wait_event(&self.wait, take, io.interrupt)
            .await
            .map_err(|_| SysErr::Restart)??;
        self.wait.wake(PollMask::WRITABLE);
        buf[..8].copy_from_slice(&value.to_ne_bytes());
        Ok(8)
    }

    /// `eventfd_write`: exactly one 8-byte count.
    async fn write_count(&self, io: Io<'_>, data: &[u8]) -> SysResult<usize> {
        let Ok(bytes) = <[u8; 8]>::try_from(data) else {
            return Err(Errno::EINVAL.into());
        };
        let value = u64::from_ne_bytes(bytes);
        if value == MAX {
            return Err(Errno::EINVAL.into());
        }
        let add = || {
            let mut count = self.count.lock_or_recover();
            if MAX - *count > value {
                *count += value;
                return Some(Ok(()));
            }
            io.nonblocking().then_some(Err(Errno::EAGAIN))
        };
        wait_event(&self.wait, add, io.interrupt)
            .await
            .map_err(|_| SysErr::Restart)??;
        if value > 0 {
            self.wait.wake(PollMask::READABLE);
        }
        Ok(8)
    }
}

impl Pollable for EventFd {
    /// `eventfd_poll`.
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.wait);
        let count = *self.count.lock_or_recover();
        let mut mask = PollMask::empty();
        if count > 0 {
            mask |= PollMask::READABLE;
        }
        if count == MAX {
            mask |= PollMask::ERR;
        }
        if MAX - 1 > count {
            mask |= PollMask::WRITABLE;
        }
        mask
    }
}

impl FileObject for EventFd {
    fn read<'a>(&'a self, io: Io<'a>, buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(self.read_count(io, buf))
    }

    fn write<'a>(&'a self, io: Io<'a>, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(self.write_count(io, data))
    }
}

impl Kernel {
    /// `do_eventfd`.
    pub(crate) async fn sys_eventfd2(
        &self,
        caller: &Caller<'_>,
        count: u32,
        flags: EventfdFlags,
    ) -> SysResult<Fd> {
        let allowed =
            EventfdFlags::EFD_SEMAPHORE | EventfdFlags::EFD_NONBLOCK | EventfdFlags::EFD_CLOEXEC;
        if !(flags & !allowed).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        let object = EventFd {
            count: Mutex::new(count.into()),
            semaphore: flags.contains(EventfdFlags::EFD_SEMAPHORE),
            wait: WaitQueue::new(),
        };
        let mut status = OpenFlags::O_RDWR;
        if flags.contains(EventfdFlags::EFD_NONBLOCK) {
            status |= OpenFlags::O_NONBLOCK;
        }
        let file = OpenFile::new(
            Backing::Stream(Arc::new(object), self.pseudo.anon("[eventfd]")),
            status,
        );
        let cloexec = flags.contains(EventfdFlags::EFD_CLOEXEC);
        let [fd] = self.install_files(caller, [file], cloexec)?;
        Ok(fd)
    }
}
