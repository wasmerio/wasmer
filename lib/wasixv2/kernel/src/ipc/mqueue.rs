//! POSIX message queues (`ipc/mqueue.c`): `mq_open`, `mq_unlink`, `mq_timedsend`,
//! `mq_timedreceive`, `mq_notify` and `mq_getsetattr`.
//!
//! A queue is a kernel object named in the namespace's queue table, which plays the part of the
//! `mqueue` filesystem's root directory: names are single components, a queue lives on after
//! `mq_unlink` until its last descriptor closes, and its owner and mode follow file creation
//! (the umask applies). A descriptor is an open file whose object is the queue, so it polls as
//! readable while messages wait and writable while there is room, and `read` returns the
//! status line Linux's `mqueue` files show. The table is not mounted anywhere, so `/dev/mqueue`
//! does not list it.
//!
//! Messages leave in priority order, oldest first within a priority. A notification fires when
//! a message arrives in an empty queue that no receiver is waiting on, then unregisters.
//! `SIGEV_THREAD` delivers musl's cookie over the netlink socket it names, as Linux does.

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::{Arc, Mutex, MutexGuard},
};

use wasixv2_abi::{
    ConstBuf, ConstKernelTimespecPtr, ConstMqAttrPtr, ConstSigeventPtr, Cstr, Mode, MqAttr,
    MqAttrPtr, OpenFlags, RlimitResource, SiCode, SigevNotify, Siginfo, U32Ptr,
};
use wasixv2_syscalls::{
    Caller, CstrError, Errno, Fd, LocalFuture, SysErr, SysResult, TaskId, read_cstr, read_value,
    write_value,
};
use wasixv2_vfs::{Cap, Cred, MAY_READ, MAY_WRITE, Node};
use wasixv2_wait::{
    PollMask, PollTable, Pollable, WaitQueue, poll_wait, wait_event, wait_event_timeout,
};

use super::{CAP_SYS_RESOURCE, capable, lock};
use crate::{
    Kernel,
    file::{Backing, FileObject, Io, OpenFile},
    netlink::NetlinkSocket,
    time,
};

/// `DFLT_QUEUESMAX` (`fs.mqueue.queues_max`): queues in the namespace.
pub(crate) const QUEUES_MAX: usize = 256;
/// `DFLT_MSGMAX` and `DFLT_MSG` (`fs.mqueue.msg_max`, `msg_default`).
pub(crate) const MSG_MAX: i32 = 10;
/// `DFLT_MSGSIZEMAX` and `DFLT_MSGSIZE` (`fs.mqueue.msgsize_max`, `msgsize_default`).
pub(crate) const MSGSIZE_MAX: i32 = 8192;
/// `HARD_MSGMAX` and `HARD_MSGSIZEMAX`, the limits with `CAP_SYS_RESOURCE`.
const HARD_MSGMAX: i32 = 65536;
const HARD_MSGSIZEMAX: i32 = 16 * 1024 * 1024;
/// `MQ_PRIO_MAX`.
const PRIO_MAX: u32 = 32768;
/// `NAME_MAX` and `PATH_MAX`.
const NAME_MAX: usize = 255;
const PATH_MAX: usize = 4096;
/// `NOTIFY_COOKIE_LEN`, and the codes its last byte carries.
const COOKIE_LEN: usize = 32;
const NOTIFY_WOKENUP: u8 = 1;
const NOTIFY_REMOVED: u8 = 2;
/// What `RLIMIT_MSGQUEUE` charges for each message slot besides its bytes: one `struct
/// msg_msg` and one `struct posix_msg_tree_node` on a 32-bit kernel.
const MSG_OVERHEAD: u64 = 24 + 24;

/// The namespace's queues, by name.
#[derive(Default)]
pub(crate) struct Mqueues {
    names: Mutex<HashMap<Vec<u8>, Arc<Mqueue>>>,
    /// Queues that exist, named or not (`mq_queues_count`).
    count: Mutex<usize>,
    /// `RLIMIT_MSGQUEUE` charges per user.
    charged: Mutex<HashMap<u32, u64>>,
}

pub(crate) struct Mqueue {
    state: Mutex<State>,
    /// Woken on every change, for senders, receivers and pollers.
    wait: WaitQueue,
    uid: u32,
    gid: u32,
    mode: u32,
    /// What the queue charged its creator's `RLIMIT_MSGQUEUE`.
    charge: u64,
    /// Its inode in the queue filesystem.
    inode: Arc<dyn Node>,
    owner: Arc<Mqueues>,
}

struct State {
    maxmsg: usize,
    msgsize: usize,
    /// Messages by priority, highest first.
    messages: BTreeMap<std::cmp::Reverse<u32>, VecDeque<Vec<u8>>>,
    curmsgs: usize,
    /// Bytes of message text queued (`qsize`).
    bytes: usize,
    /// Receivers blocked in `mq_timedreceive`, which take a message before any notification.
    receivers: usize,
    notify: Option<Notify>,
}

/// A registration made with `mq_notify`.
struct Notify {
    /// The thread group that registered.
    owner: u32,
    how: How,
}

enum How {
    None,
    Signal {
        signo: i32,
        value: [u8; 4],
    },
    Thread {
        socket: Arc<NetlinkSocket>,
        cookie: Vec<u8>,
    },
}

impl State {
    fn pop(&mut self) -> Option<(u32, Vec<u8>)> {
        let mut entry = self.messages.first_entry()?;
        let prio = entry.key().0;
        let message = entry
            .get_mut()
            .pop_front()
            .expect("no empty priority lists");
        if entry.get().is_empty() {
            entry.remove();
        }
        self.curmsgs -= 1;
        self.bytes -= message.len();
        Some((prio, message))
    }

    /// `remove_notification`: a `SIGEV_THREAD` registration learns it is gone.
    fn remove_notification(&mut self) {
        if let Some(Notify {
            how: How::Thread { socket, mut cookie },
            ..
        }) = self.notify.take()
        {
            cookie[COOKIE_LEN - 1] = NOTIFY_REMOVED;
            socket.deliver(cookie);
        }
    }
}

impl Mqueue {
    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    /// `inode_permission` on the queue's inode.
    fn permits(&self, cred: &Cred, mask: u32) -> bool {
        let bits = if cred.fsuid == self.uid {
            self.mode >> 6
        } else if cred.in_group(self.gid) {
            self.mode >> 3
        } else {
            self.mode
        } & 7;
        mask & !bits == 0 || cred.capable(Cap::DacOverride)
    }
}

impl Drop for Mqueue {
    /// `mqueue_evict_inode`: the last reference is gone; the queue no longer counts.
    fn drop(&mut self) {
        *lock(&self.owner.count) -= 1;
        if let Some(charged) = lock(&self.owner.charged).get_mut(&self.uid) {
            *charged -= self.charge;
        }
        self.state
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove_notification();
    }
}

/// An open queue: what a message queue descriptor refers to.
pub(crate) struct MqFile {
    queue: Arc<Mqueue>,
    /// Where `read` of the status line is.
    read_pos: Mutex<usize>,
}

impl MqFile {
    /// `mqueue_read_file`'s status line.
    fn status(&self) -> Vec<u8> {
        let state = self.queue.state();
        let (notify, signo, pid) = match &state.notify {
            None => (0, 0, 0),
            Some(notify) => match notify.how {
                How::None => (1, 0, notify.owner),
                How::Signal { signo, .. } => (0, signo, notify.owner),
                How::Thread { .. } => (2, 0, notify.owner),
            },
        };
        format!(
            "QSIZE:{:<10} NOTIFY:{:<5} SIGNO:{:<5} NOTIFY_PID:{:<6}\n",
            state.bytes, notify, signo, pid
        )
        .into_bytes()
    }
}

impl Pollable for MqFile {
    /// `mqueue_poll_file`.
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.queue.wait);
        let state = self.queue.state();
        let mut mask = PollMask::empty();
        if state.curmsgs > 0 {
            mask |= PollMask::READABLE;
        }
        if state.curmsgs < state.maxmsg {
            mask |= PollMask::WRITABLE;
        }
        mask
    }
}

impl FileObject for MqFile {
    fn read<'a>(&'a self, _io: Io<'a>, buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        let status = self.status();
        let mut pos = lock(&self.read_pos);
        let rest = status.get(*pos..).unwrap_or_default();
        let n = rest.len().min(buf.len());
        buf[..n].copy_from_slice(&rest[..n]);
        *pos += n;
        Box::pin(async move { Ok(n) })
    }

    /// The queue's file has no `write` operation.
    fn write<'a>(&'a self, _io: Io<'a>, _data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EINVAL.into()) })
    }

    fn can_poll(&self) -> bool {
        true
    }
}

/// A receiver counted as waiting for the duration of a `mq_timedreceive`, so a notification
/// does not take the message it waits for.
struct Receiving<'a>(&'a Mqueue);

impl<'a> Receiving<'a> {
    fn new(queue: &'a Mqueue) -> Self {
        queue.state().receivers += 1;
        Self(queue)
    }
}

impl Drop for Receiving<'_> {
    fn drop(&mut self) {
        self.0.state().receivers -= 1;
    }
}

/// `lookup_noperm_common` and `simple_lookup` on a queue name. musl strips the leading `/`.
fn check_name(name: &[u8]) -> Result<(), Errno> {
    if name.is_empty() || name == b"." || name == b".." || name.contains(&b'/') {
        return Err(Errno::EACCES);
    }
    if name.len() > NAME_MAX {
        return Err(Errno::ENAMETOOLONG);
    }
    Ok(())
}

async fn read_name(caller: &Caller<'_>, name: Cstr) -> Result<Vec<u8>, Errno> {
    match read_cstr(caller.memory(), name, PATH_MAX).await {
        Ok(name) => Ok(name),
        Err(CstrError::Fault) => Err(Errno::EFAULT),
        Err(CstrError::TooLong) => Err(Errno::ENAMETOOLONG),
    }
}

/// How long a send or receive may block: until an absolute `CLOCK_REALTIME` time.
type Deadline = Option<std::time::Duration>;

async fn read_deadline(caller: &Caller<'_>, ts: ConstKernelTimespecPtr) -> SysResult<Deadline> {
    if ts.is_null() {
        return Ok(None);
    }
    let ts = read_value(caller.memory(), ts).await?;
    Ok(Some(time::from_timespec(ts)?))
}

impl Kernel {
    /// The queue behind descriptor `fd` and its open file; `EBADF` for anything else.
    fn mq_file(&self, caller: &Caller<'_>, fd: Fd) -> Result<(Arc<OpenFile>, Arc<Mqueue>), Errno> {
        let file = self.file(caller.task(), fd)?;
        let queue = file.downcast::<MqFile>().ok_or(Errno::EBADF)?.queue.clone();
        Ok((file, queue))
    }

    pub(crate) async fn sys_mq_open(
        &self,
        caller: &Caller<'_>,
        name: Cstr,
        oflag: OpenFlags,
        mode: Mode,
        attr: ConstMqAttrPtr,
    ) -> SysResult<i32> {
        let attr = match attr.is_null() {
            true => None,
            false => Some(read_value(caller.memory(), attr).await?),
        };
        let name = read_name(caller, name).await?;
        // The descriptor comes first (`FD_ADD`): no queue is made for a full table.
        if !self.files(caller.task()).lock().unwrap().has_room(1) {
            return Err(Errno::EMFILE.into());
        }
        check_name(&name)?;
        let env = self.env(caller);
        let cred = &env.cred;
        let mqueues = &self.ipc.mq;
        let queue = {
            let mut names = lock(&mqueues.names);
            match names.get(&name) {
                None => {
                    if !oflag.contains(OpenFlags::O_CREAT) {
                        return Err(Errno::ENOENT.into());
                    }
                    let mode = mode & !env.fs.umask & 0o777;
                    let limit = self
                        .process(caller.task())
                        .resources
                        .lock()
                        .unwrap()
                        .soft(RlimitResource::RLIMIT_MSGQUEUE);
                    let queue = self.mq_create(cred, mode, attr, limit)?;
                    names.insert(name.clone(), queue.clone());
                    queue
                }
                Some(queue) => {
                    if oflag.contains(OpenFlags::O_CREAT | OpenFlags::O_EXCL) {
                        return Err(Errno::EEXIST.into());
                    }
                    let mask = match oflag & OpenFlags::O_ACCMODE {
                        OpenFlags::O_RDONLY => MAY_READ,
                        OpenFlags::O_WRONLY => MAY_WRITE,
                        OpenFlags::O_RDWR => MAY_READ | MAY_WRITE,
                        _ => return Err(Errno::EINVAL.into()),
                    };
                    if !queue.permits(cred, mask) {
                        return Err(Errno::EACCES.into());
                    }
                    queue.clone()
                }
            }
        };
        let object = MqFile {
            queue,
            read_pos: Mutex::new(0),
        };
        let kept = OpenFlags::O_ACCMODE | OpenFlags::O_NONBLOCK;
        let path = self.pseudo.mqueue(object.queue.inode.clone(), &name);
        let file = OpenFile::new(Backing::Stream(Arc::new(object), path), oflag & kept);
        let [fd] = self.install_files(caller, [file], true)?;
        Ok(fd.0)
    }

    /// `mqueue_get_inode` for a new queue, with `mqueue_create_attr`'s count limit.
    fn mq_create(
        &self,
        cred: &Cred,
        mode: u32,
        attr: Option<MqAttr>,
        limit: u64,
    ) -> Result<Arc<Mqueue>, Errno> {
        let mqueues = &self.ipc.mq;
        let resource = capable(cred, CAP_SYS_RESOURCE);
        let mut count = lock(&mqueues.count);
        if *count >= QUEUES_MAX && !resource {
            return Err(Errno::ENOSPC);
        }
        let (maxmsg, msgsize) = match attr {
            Some(attr) => (attr.mq_maxmsg, attr.mq_msgsize),
            None => (MSG_MAX, MSGSIZE_MAX),
        };
        if maxmsg <= 0 || msgsize <= 0 {
            return Err(Errno::EINVAL);
        }
        let (max_msg, max_size) = match resource {
            true => (HARD_MSGMAX, HARD_MSGSIZEMAX),
            false => (MSG_MAX, MSGSIZE_MAX),
        };
        if maxmsg > max_msg || msgsize > max_size {
            return Err(Errno::EINVAL);
        }
        let (maxmsg, msgsize) = (maxmsg as u64, msgsize as u64);
        let charge = maxmsg * (msgsize + MSG_OVERHEAD);
        {
            let mut charged = lock(&mqueues.charged);
            let user = charged.entry(cred.uid).or_default();
            if *user + charge > limit {
                return Err(Errno::EMFILE);
            }
            *user += charge;
        }
        *count += 1;
        Ok(Arc::new(Mqueue {
            state: Mutex::new(State {
                maxmsg: maxmsg as usize,
                msgsize: msgsize as usize,
                messages: BTreeMap::new(),
                curmsgs: 0,
                bytes: 0,
                receivers: 0,
                notify: None,
            }),
            wait: WaitQueue::new(),
            uid: cred.fsuid,
            gid: cred.fsgid,
            mode,
            charge,
            inode: self.pseudo.mqueue_inode((cred.fsuid, cred.fsgid), mode),
            owner: mqueues.clone(),
        }))
    }

    pub(crate) async fn sys_mq_unlink(&self, caller: &Caller<'_>, name: Cstr) -> SysResult<i32> {
        let name = read_name(caller, name).await?;
        check_name(&name)?;
        let cred = self.env(caller).cred;
        let mut names = lock(&self.ipc.mq.names);
        let queue = names.get(&name).ok_or(Errno::ENOENT)?;
        // `may_delete` in the mqueue root, which is root's and sticky (`S_ISVTX`): only the
        // queue's owner, root or `CAP_FOWNER` may remove a queue.
        if cred.fsuid != queue.uid && cred.fsuid != 0 && !cred.capable(Cap::Fowner) {
            return Err(Errno::EPERM.into());
        }
        names.remove(&name);
        Ok(0)
    }

    pub(crate) async fn sys_mq_timedsend(
        &self,
        caller: &Caller<'_>,
        mqdes: Fd,
        msg: ConstBuf,
        len: u32,
        prio: u32,
        timeout: ConstKernelTimespecPtr,
    ) -> SysResult<i32> {
        let deadline = read_deadline(caller, timeout).await?;
        if prio >= PRIO_MAX {
            return Err(Errno::EINVAL.into());
        }
        let (file, queue) = self.mq_file(caller, mqdes)?;
        file.check_writable()?;
        if len as usize > queue.state().msgsize {
            return Err(Errno::EMSGSIZE.into());
        }
        let mut text = vec![0; len as usize];
        caller.memory().read(msg.addr(), &mut text).await?;
        let nonblocking = file.flags().contains(OpenFlags::O_NONBLOCK);
        let sender = (self.process(caller.task()).pid.0, self.env(caller).cred.uid);
        let mut message = Some(text);
        let send = || -> Option<Result<(), Errno>> {
            let mut state = queue.state();
            if state.curmsgs == state.maxmsg {
                return nonblocking.then_some(Err(Errno::EAGAIN));
            }
            let text = message.take().expect("sent once");
            state.bytes += text.len();
            state.curmsgs += 1;
            state
                .messages
                .entry(std::cmp::Reverse(prio))
                .or_default()
                .push_back(text);
            if state.curmsgs == 1 && state.receivers == 0 {
                self.mq_notify_owner(&mut state, sender);
            }
            drop(state);
            queue.wait.wake(PollMask::READABLE);
            Some(Ok(()))
        };
        self.mq_wait(caller, &queue.wait, send, deadline).await?;
        Ok(0)
    }

    pub(crate) async fn sys_mq_timedreceive(
        &self,
        caller: &Caller<'_>,
        mqdes: Fd,
        msg: wasixv2_abi::Buf,
        len: u32,
        prio: U32Ptr,
        timeout: ConstKernelTimespecPtr,
    ) -> SysResult<i32> {
        let deadline = read_deadline(caller, timeout).await?;
        let (file, queue) = self.mq_file(caller, mqdes)?;
        file.check_readable()?;
        if (len as usize) < queue.state().msgsize {
            return Err(Errno::EMSGSIZE.into());
        }
        let nonblocking = file.flags().contains(OpenFlags::O_NONBLOCK);
        let _waiting = Receiving::new(&queue);
        let receive = || -> Option<Result<(u32, Vec<u8>), Errno>> {
            let mut state = queue.state();
            match state.pop() {
                Some(message) => {
                    drop(state);
                    queue.wait.wake(PollMask::WRITABLE);
                    Some(Ok(message))
                }
                None => nonblocking.then_some(Err(Errno::EAGAIN)),
            }
        };
        let (message_prio, text) = self.mq_wait(caller, &queue.wait, receive, deadline).await?;
        if !prio.is_null() {
            write_value(caller.memory(), prio, &message_prio).await?;
        }
        caller.memory().write(msg.addr(), &text).await?;
        Ok(text.len() as i32)
    }

    /// `wq_sleep`: waits for `ready` until the absolute `CLOCK_REALTIME` deadline
    /// (`ETIMEDOUT`) or a signal (restarted with `SA_RESTART`).
    async fn mq_wait<T>(
        &self,
        caller: &Caller<'_>,
        queue: &WaitQueue,
        ready: impl FnMut() -> Option<Result<T, Errno>>,
        deadline: Deadline,
    ) -> SysResult<T> {
        let interrupt = self.interrupt(caller);
        let woken = match deadline {
            None => wait_event(queue, ready, &*interrupt).await.map(Some),
            Some(at) => {
                let sleep = self.host.sleep(at.saturating_sub(self.host.realtime()));
                wait_event_timeout(queue, ready, &*interrupt, sleep).await
            }
        };
        match woken {
            Ok(Some(result)) => Ok(result?),
            Ok(None) => Err(Errno::ETIMEDOUT.into()),
            Err(_) => Err(SysErr::Restart),
        }
    }

    /// `__do_notify`: the queue went from empty to not empty with nobody waiting.
    fn mq_notify_owner(&self, state: &mut State, (pid, uid): (u32, u32)) {
        let Some(notify) = state.notify.take() else {
            return;
        };
        match notify.how {
            How::None => {}
            How::Signal { signo, value } => {
                let mut info = Siginfo {
                    si_signo: signo,
                    si_errno: 0,
                    si_code: SiCode::SI_MESGQ.0,
                    sifields: [0; 116],
                };
                info.sifields[0..4].copy_from_slice(&pid.to_le_bytes());
                info.sifields[4..8].copy_from_slice(&uid.to_le_bytes());
                info.sifields[8..12].copy_from_slice(&value);
                // The owner may be gone; nobody hears of it then.
                let _ = self.send_to_group(None, notify.owner as i32, info);
            }
            How::Thread { socket, mut cookie } => {
                cookie[COOKIE_LEN - 1] = NOTIFY_WOKENUP;
                socket.deliver(cookie);
            }
        }
    }

    pub(crate) async fn sys_mq_notify(
        &self,
        caller: &Caller<'_>,
        mqdes: Fd,
        notification: ConstSigeventPtr,
    ) -> SysResult<i32> {
        let how = match notification.is_null() {
            true => None,
            false => {
                let sev = read_value(caller.memory(), notification).await?;
                Some(match SigevNotify(sev.sigev_notify) {
                    SigevNotify::SIGEV_NONE => How::None,
                    SigevNotify::SIGEV_SIGNAL => {
                        if !(1..=crate::signal::NSIG).contains(&sev.sigev_signo) {
                            return Err(Errno::EINVAL.into());
                        }
                        How::Signal {
                            signo: sev.sigev_signo,
                            value: sev.sigev_value.0,
                        }
                    }
                    SigevNotify::SIGEV_THREAD => {
                        let at = u32::from_le_bytes(sev.sigev_value.0);
                        let mut cookie = vec![0; COOKIE_LEN];
                        caller.memory().read(at, &mut cookie).await?;
                        let sock = self.file(caller.task(), Fd(sev.sigev_signo))?;
                        let socket = sock
                            .object()
                            .cloned()
                            .and_then(crate::netlink::downcast)
                            .ok_or(Errno::ENOTSOCK)?;
                        How::Thread { socket, cookie }
                    }
                    _ => return Err(Errno::EINVAL.into()),
                })
            }
        };
        let (_, queue) = self.mq_file(caller, mqdes)?;
        let me = self.process(caller.task()).pid.0;
        let mut state = queue.state();
        if state
            .notify
            .as_ref()
            .is_some_and(|n| !self.process_alive(n.owner))
        {
            // The owner went away without closing the queue through us.
            state.remove_notification();
        }
        match how {
            None => {
                if state.notify.as_ref().is_some_and(|n| n.owner == me) {
                    state.remove_notification();
                }
            }
            Some(_) if state.notify.is_some() => return Err(Errno::EBUSY.into()),
            Some(how) => state.notify = Some(Notify { owner: me, how }),
        }
        Ok(0)
    }

    pub(crate) async fn sys_mq_getsetattr(
        &self,
        caller: &Caller<'_>,
        mqdes: Fd,
        new: ConstMqAttrPtr,
        old: MqAttrPtr,
    ) -> SysResult<i32> {
        let new = match new.is_null() {
            true => None,
            false => Some(read_value(caller.memory(), new).await?),
        };
        if new.is_some_and(|new| new.mq_flags as u32 & !OpenFlags::O_NONBLOCK.0 != 0) {
            return Err(Errno::EINVAL.into());
        }
        let (file, queue) = self.mq_file(caller, mqdes)?;
        let current = {
            let state = queue.state();
            MqAttr {
                mq_flags: (file.flags() & OpenFlags::O_NONBLOCK).0 as i32,
                mq_maxmsg: state.maxmsg as i32,
                mq_msgsize: state.msgsize as i32,
                mq_curmsgs: state.curmsgs as i32,
                ..MqAttr::default()
            }
        };
        if let Some(new) = new {
            let flags = file.flags() & !OpenFlags::O_NONBLOCK;
            file.set_status_flags(flags | OpenFlags(new.mq_flags as u32 & OpenFlags::O_NONBLOCK.0));
        }
        if !old.is_null() {
            write_value(caller.memory(), old, &current).await?;
        }
        Ok(0)
    }

    /// `mqueue_flush_file`: closing any descriptor of a queue drops the notification of the
    /// closing thread group, `pid`.
    pub(crate) fn mq_flush(&self, pid: TaskId, file: &OpenFile) {
        let Some(mq) = file.downcast::<MqFile>() else {
            return;
        };
        let me = pid.0;
        let mut state = mq.queue.state();
        if state.notify.as_ref().is_some_and(|n| n.owner == me) {
            state.remove_notification();
        }
    }

    /// Whether thread group `pid` still has a task.
    fn process_alive(&self, pid: u32) -> bool {
        let state = self.state.lock().unwrap();
        state
            .tasks
            .values()
            .any(|t| t.process.pid.0 == pid && !t.process.group.exiting())
    }
}
