//! Seccomp user notifications (`SECCOMP_RET_USER_NOTIF`): a filter installed with
//! `SECCOMP_FILTER_FLAG_NEW_LISTENER` gives a supervisor a listener descriptor, and a call the
//! filter answers with `SECCOMP_RET_USER_NOTIF` waits until the supervisor reads it
//! (`SECCOMP_IOCTL_NOTIF_RECV`) and answers it (`SECCOMP_IOCTL_NOTIF_SEND`), possibly with a
//! descriptor of its own (`SECCOMP_IOCTL_NOTIF_ADDFD`) or by letting the call run
//! (`SECCOMP_USER_NOTIF_FLAG_CONTINUE`). A filter without a listener, or whose listener is
//! closed, fails the call with `ENOSYS`.

use std::sync::{Arc, Mutex, OnceLock, Weak};

use wasixv2_abi::{
    Errno, GuestLayout, GuestPtr, GuestPtrMut, IoctlRequest, OpenFlags, SeccompData, SeccompNotif,
    SeccompNotifAddfd, SeccompNotifFlags, SeccompNotifResp,
};
use wasixv2_syscalls::{
    Caller, Fd, LocalFuture, PidView as _, SysErr, SysResult, TaskId, read_value, write_value,
};
use wasixv2_wait::{Interrupt, PollMask, PollTable, Pollable, WaitQueue, poll_wait, wait_event};

use super::Filter;
use crate::{
    Kernel,
    file::{FileObject, Io},
    sync::MutexExt,
};

/// A filter's notifications (`struct notification`).
pub(crate) struct Notif {
    state: Mutex<State>,
    /// Woken when a notification arrives or is answered, and when the listener closes.
    wait: WaitQueue,
    /// `SECCOMP_FILTER_FLAG_WAIT_KILLABLE_RECV`: once the supervisor has read it, only a
    /// fatal signal ends a task's wait.
    killable_recv: bool,
    /// The filter while tasks use it (`filter->users`).
    filter: OnceLock<Weak<Filter>>,
}

struct State {
    next_id: u64,
    list: Vec<Knotif>,
    /// The listener is gone: every notification is `ENOSYS`.
    closed: bool,
}

/// One task's notification (`struct seccomp_knotif`).
struct Knotif {
    id: u64,
    task: TaskId,
    data: SeccompData,
    state: Reply,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Reply {
    /// `SECCOMP_NOTIFY_INIT`: not read yet.
    Waiting,
    /// `SECCOMP_NOTIFY_SENT`: read, not answered.
    Read,
    /// `SECCOMP_NOTIFY_REPLIED`.
    Answered { error: i32, val: i64, cont: bool },
}

impl Notif {
    /// A listener's notifications, numbered from `first`, a random id as Linux starts with.
    pub(crate) fn new(first: u64, killable_recv: bool) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                next_id: first,
                list: Vec::new(),
                closed: false,
            }),
            wait: WaitQueue::new(),
            killable_recv,
            filter: OnceLock::new(),
        })
    }

    /// Ties the notifications to the filter they are for, once it exists.
    pub(crate) fn set_filter(&self, filter: &Arc<Filter>) {
        let _ = self.filter.set(Arc::downgrade(filter));
    }

    /// `seccomp_notify_detach`: the listener closed; what waits gets `ENOSYS`.
    fn detach(&self) {
        let mut st = self.state.lock_or_recover();
        st.closed = true;
        for n in &mut st.list {
            n.state = Reply::Answered {
                error: -Errno::ENOSYS.0,
                val: 0,
                cont: false,
            };
        }
        drop(st);
        self.wait.wake(PollMask::empty());
    }
}

/// The listener descriptor's object (`seccomp_notify_ops`).
pub(crate) struct Listener(pub(crate) Arc<Notif>);

impl Drop for Listener {
    fn drop(&mut self) {
        self.0.detach();
    }
}

impl Pollable for Listener {
    /// `seccomp_notify_poll`.
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.0.wait);
        let st = self.0.state.lock_or_recover();
        let mut mask = PollMask::empty();
        if st.list.iter().any(|n| n.state == Reply::Waiting) {
            mask |= PollMask::READABLE;
        }
        if st.list.iter().any(|n| n.state == Reply::Read) {
            mask |= PollMask::WRITABLE;
        }
        if self.0.filter.get().is_some_and(|f| f.strong_count() == 0) {
            mask |= PollMask::HUP;
        }
        mask
    }
}

impl FileObject for Listener {
    fn read<'a>(&'a self, _: Io<'a>, _: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EINVAL.into()) })
    }

    fn write<'a>(&'a self, _: Io<'a>, _: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EINVAL.into()) })
    }
}

/// What a notified call comes to.
pub(super) enum Outcome {
    /// `SECCOMP_USER_NOTIF_FLAG_CONTINUE`.
    Run,
    /// The raw result register.
    Return(i64),
}

impl Kernel {
    /// `seccomp_do_user_notification`: queues `data` for `notif`'s supervisor and waits for
    /// the answer. A signal while it waits withdraws it and restarts the call; with
    /// `SECCOMP_FILTER_FLAG_WAIT_KILLABLE_RECV`, only until the supervisor read it.
    pub(super) async fn seccomp_notify(
        &self,
        caller: &Caller<'_>,
        notif: &Arc<Notif>,
        data: SeccompData,
    ) -> SysResult<Outcome> {
        let id = {
            let mut st = notif.state.lock_or_recover();
            if st.closed {
                return Ok(Outcome::Return(-i64::from(Errno::ENOSYS.0)));
            }
            let id = st.next_id;
            st.next_id = st.next_id.wrapping_add(1);
            st.list.push(Knotif {
                id,
                task: caller.task(),
                data,
                state: Reply::Waiting,
            });
            id
        };
        notif.wait.wake(PollMask::READABLE);
        let signals = self.interrupt(caller);
        let state = |st: &State| st.list.iter().find(|n| n.id == id).map(|n| n.state);
        let answered = || match state(&notif.state.lock_or_recover()) {
            Some(Reply::Answered { error, val, cont }) => Some((error, val, cont)),
            _ => None,
        };
        let mut result = wait_event(&notif.wait, answered, &*signals).await;
        // Read already: a supervisor that took it is waited for unless the task is killed.
        if result.is_err()
            && notif.killable_recv
            && state(&notif.state.lock_or_recover()) == Some(Reply::Read)
        {
            let killable = signals.killable();
            result = wait_event(&notif.wait, answered, &killable as &dyn Interrupt).await;
        }
        let mut st = notif.state.lock_or_recover();
        st.list.retain(|n| n.id != id);
        drop(st);
        notif.wait.wake(PollMask::WRITABLE);
        match result {
            Err(_) => Err(SysErr::Restart),
            Ok((_, _, true)) => Ok(Outcome::Run),
            Ok((0, val, false)) => Ok(Outcome::Return(val)),
            Ok((error, _, false)) => Ok(Outcome::Return(i64::from(error))),
        }
    }

    /// The listener's ioctls (`seccomp_notify_ioctl`).
    pub(crate) async fn seccomp_notify_ioctl(
        &self,
        caller: &Caller<'_>,
        listener: &Listener,
        request: IoctlRequest,
        arg: u32,
    ) -> SysResult<i32> {
        let notif = &listener.0;
        let memory = caller.memory();
        match request {
            IoctlRequest::SECCOMP_IOCTL_NOTIF_RECV => {
                let mut zero = [0; SeccompNotif::SIZE];
                memory.read(arg, &mut zero).await?;
                if zero.iter().any(|b| *b != 0) {
                    return Err(Errno::EINVAL.into());
                }
                let signals = self.interrupt(caller);
                let take = || {
                    let mut st = notif.state.lock_or_recover();
                    let n = st.list.iter_mut().find(|n| n.state == Reply::Waiting)?;
                    n.state = Reply::Read;
                    Some((n.id, n.task, n.data))
                };
                let (id, task, data) = wait_event(&notif.wait, take, &*signals)
                    .await
                    .map_err(|_| SysErr::Restart)?;
                let out = SeccompNotif {
                    id,
                    pid: self.pid_in(caller.task(), task) as u32,
                    flags: 0,
                    data,
                };
                if write_value(memory, GuestPtrMut::new(arg), &out)
                    .await
                    .is_err()
                {
                    // Kept for another read.
                    let mut st = notif.state.lock_or_recover();
                    if let Some(n) = st.list.iter_mut().find(|n| n.id == id) {
                        n.state = Reply::Waiting;
                    }
                    return Err(Errno::EFAULT.into());
                }
                notif.wait.wake(PollMask::WRITABLE);
                Ok(0)
            }
            IoctlRequest::SECCOMP_IOCTL_NOTIF_SEND => {
                let resp: SeccompNotifResp = read_value(memory, GuestPtr::new(arg)).await?;
                let cont = SeccompNotifFlags::SECCOMP_USER_NOTIF_FLAG_CONTINUE.0;
                if resp.flags & !cont != 0 {
                    return Err(Errno::EINVAL.into());
                }
                let cont = resp.flags & cont != 0;
                if cont && (resp.error != 0 || resp.val != 0) {
                    return Err(Errno::EINVAL.into());
                }
                let reply = Reply::Answered {
                    error: resp.error,
                    val: resp.val,
                    cont,
                };
                self.answer(notif, resp.id, reply)?;
                Ok(0)
            }
            IoctlRequest::SECCOMP_IOCTL_NOTIF_ID_VALID => {
                let id: u64 = read_value(memory, GuestPtr::new(arg)).await?;
                let st = notif.state.lock_or_recover();
                match st.list.iter().any(|n| n.id == id && n.state == Reply::Read) {
                    true => Ok(0),
                    false => Err(Errno::ENOENT.into()),
                }
            }
            // A scheduling hint; the value is the flags themselves.
            IoctlRequest::SECCOMP_IOCTL_NOTIF_SET_FLAGS => {
                let sync = SeccompNotifFlags::SECCOMP_USER_NOTIF_FD_SYNC_WAKE_UP.0;
                match arg & !sync {
                    0 => Ok(0),
                    _ => Err(Errno::EINVAL.into()),
                }
            }
            IoctlRequest::SECCOMP_IOCTL_NOTIF_ADDFD => self.notify_addfd(caller, notif, arg).await,
            _ => Err(Errno::EINVAL.into()),
        }
    }

    /// Answers the read notification `id` (`ENOENT` for none, `EINPROGRESS` if not read or
    /// answered already).
    fn answer(&self, notif: &Notif, id: u64, reply: Reply) -> Result<(), Errno> {
        let mut st = notif.state.lock_or_recover();
        let n = st
            .list
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or(Errno::ENOENT)?;
        if n.state != Reply::Read {
            return Err(Errno::EINPROGRESS);
        }
        n.state = reply;
        drop(st);
        notif.wait.wake(PollMask::empty());
        Ok(())
    }

    /// `seccomp_notify_addfd`: the supervisor's descriptor `srcfd` installed in the notified
    /// task, at `newfd` with `SECCOMP_ADDFD_FLAG_SETFD`, and with `SECCOMP_ADDFD_FLAG_SEND`
    /// as the answer to its call. Returns the descriptor's number there.
    async fn notify_addfd(&self, caller: &Caller<'_>, notif: &Notif, arg: u32) -> SysResult<i32> {
        let addfd: SeccompNotifAddfd = read_value(caller.memory(), GuestPtr::new(arg)).await?;
        let setfd = SeccompNotifFlags::SECCOMP_ADDFD_FLAG_SETFD.0;
        let send = SeccompNotifFlags::SECCOMP_ADDFD_FLAG_SEND.0;
        if addfd.flags & !(setfd | send) != 0
            || addfd.newfd_flags & !OpenFlags::O_CLOEXEC.0 != 0
            || (addfd.flags & setfd == 0 && addfd.newfd != 0)
        {
            return Err(Errno::EINVAL.into());
        }
        let file = self.file(caller.task(), Fd(addfd.srcfd as i32))?;
        let task = {
            let st = notif.state.lock_or_recover();
            let n = st
                .list
                .iter()
                .find(|n| n.id == addfd.id)
                .ok_or(Errno::ENOENT)?;
            if n.state != Reply::Read {
                return Err(Errno::EINPROGRESS.into());
            }
            n.task
        };
        let cloexec = addfd.newfd_flags & OpenFlags::O_CLOEXEC.0 != 0;
        let files = self.files(task);
        let mut table = files.lock_or_recover();
        let fd = match addfd.flags & setfd != 0 {
            // `replace_fd`.
            true => {
                let fd = Fd(addfd.newfd as i32);
                if fd.0 < 0 || fd.0 as usize >= table.limit() {
                    return Err(Errno::EBADF.into());
                }
                table.expand(fd.0 as usize).map_err(|_| Errno::EBADF)?;
                if table.is_reserved(fd) {
                    return Err(Errno::EBUSY.into());
                }
                if let Some(replaced) = table.install_at(fd, file, cloexec) {
                    drop(table);
                    self.closed_by(self.process(task).pid, &replaced);
                }
                fd
            }
            false => table.install(file, cloexec).ok_or(Errno::EMFILE)?,
        };
        if addfd.flags & send != 0 {
            let reply = Reply::Answered {
                error: 0,
                val: i64::from(fd.0),
                cont: false,
            };
            self.answer(notif, addfd.id, reply)?;
        }
        Ok(fd.0)
    }
}
