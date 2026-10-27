//! System V message queues (`ipc/msg.c`): `msgget`, `msgsnd`, `msgrcv` and `msgctl`.
//!
//! Messages are copied into the kernel on send and out on receive, so queues work between any
//! processes. A sender waits while the queue is full and a receiver while no message matches;
//! both re-check after every change to the queue, which gives the same results as Linux's direct
//! hand-off to a waiting receiver (`pipelined_send`) except that a message never bypasses the
//! queue's byte limit.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard},
};

use wasixv2_abi::{
    ConstMsgbufPtr, GuestPtr, GuestPtrMut, IpcCmd, IpcFlags, MsgbufPtr, MsgrcvFlags, Msqid64Ds,
};
use wasixv2_syscalls::{Caller, Errno, SysErr, SysResult, read_value, write_value};
use wasixv2_wait::{PollMask, WaitQueue, wait_event};

use super::{CAP_SYS_RESOURCE, Ids, Object, Perm, READ, WRITE, capable, get_or_create, lock};
use crate::Kernel;

/// `MSGMNI`: queues in the namespace.
pub(crate) const MSGMNI: i32 = 32000;
/// `MSGMAX`: bytes in one message.
pub(crate) const MSGMAX: i32 = 8192;
/// `MSGMNB`: a new queue's byte limit.
pub(crate) const MSGMNB: i32 = 16384;
const MSGPOOL: i32 = MSGMNI * MSGMNB / 1024;
const MSGTQL: i32 = MSGMNB;
const MSGMAP: i32 = MSGMNB;
const MSGSSZ: i32 = 16;
const MSGSEG: u16 = 0xffff;

/// Where the text of the `struct msgbuf` at `msgp` starts, after its `long mtype`. A buffer at
/// the top of memory faults, as reading on past it would.
fn mtext(msgp: u32) -> Result<u32, Errno> {
    msgp.checked_add(4).ok_or(Errno::EFAULT)
}

/// The namespace's message queues.
#[derive(Default)]
pub(crate) struct Queues {
    ids: Mutex<Ids<MsgQueue>>,
}

pub(crate) struct MsgQueue {
    state: Mutex<State>,
    perm: Mutex<Perm>,
    /// Woken on every change to the queue, for senders and receivers alike.
    queue: WaitQueue,
}

impl Object for MsgQueue {
    fn perm(&self) -> MutexGuard<'_, Perm> {
        lock(&self.perm)
    }
}

struct State {
    removed: bool,
    messages: VecDeque<Message>,
    cbytes: usize,
    qbytes: usize,
    lspid: u32,
    lrpid: u32,
    stime: u64,
    rtime: u64,
    ctime: u64,
}

struct Message {
    mtype: i32,
    text: Vec<u8>,
}

/// How `msgrcv` picks a message (`convert_mode`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Search {
    Any,
    Equal(i32),
    NotEqual(i32),
    /// The lowest type up to this one.
    LessEqual(i32),
    /// `MSG_COPY`: the message at this position.
    Number(i32),
}

impl State {
    /// `msg_fits_inqueue`.
    fn fits(&self, size: usize) -> bool {
        size + self.cbytes <= self.qbytes && self.messages.len() < self.qbytes
    }

    /// `find_msg`: the position of the message `search` selects.
    fn find(&self, search: Search) -> Option<usize> {
        let mut found = None;
        let mut bound = match search {
            Search::LessEqual(bound) => bound,
            _ => 0,
        };
        for (at, message) in self.messages.iter().enumerate() {
            match search {
                Search::Any => return Some(at),
                Search::Equal(t) if message.mtype == t => return Some(at),
                Search::NotEqual(t) if message.mtype != t => return Some(at),
                Search::Number(n) if at as i32 == n => return Some(at),
                Search::LessEqual(_) if message.mtype <= bound => {
                    found = Some(at);
                    if message.mtype == 1 {
                        return found;
                    }
                    bound = message.mtype - 1;
                }
                _ => {}
            }
        }
        found
    }

    fn to_abi(&self, perm: &Perm) -> Msqid64Ds {
        let (stime, stime_high) = super::split_time(self.stime);
        let (rtime, rtime_high) = super::split_time(self.rtime);
        let (ctime, ctime_high) = super::split_time(self.ctime);
        Msqid64Ds {
            msg_perm: perm.to_abi(),
            msg_stime: stime,
            msg_stime_high: stime_high,
            msg_rtime: rtime,
            msg_rtime_high: rtime_high,
            msg_ctime: ctime,
            msg_ctime_high: ctime_high,
            msg_cbytes: self.cbytes as u32,
            msg_qnum: self.messages.len() as u32,
            msg_qbytes: self.qbytes as u32,
            msg_lspid: self.lspid as i32,
            msg_lrpid: self.lrpid as i32,
            ..Msqid64Ds::default()
        }
    }
}

impl MsgQueue {
    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }
}

impl Queues {
    fn get(&self, id: i32) -> Result<Arc<MsgQueue>, Errno> {
        lock(&self.ids).get(id).ok_or(Errno::EINVAL)
    }

    /// Rows of `/proc/sysvipc/msg`.
    #[allow(
        dead_code,
        reason = "the provider for /proc/sysvipc/msg, for procfs to read"
    )]
    pub(crate) fn list(&self) -> Vec<MsgInfo> {
        lock(&self.ids)
            .all()
            .into_iter()
            .map(|(id, queue)| {
                let state = queue.state();
                MsgInfo {
                    id,
                    perm: queue.perm().clone(),
                    cbytes: state.cbytes,
                    qnum: state.messages.len(),
                    lspid: state.lspid,
                    lrpid: state.lrpid,
                    stime: state.stime,
                    rtime: state.rtime,
                    ctime: state.ctime,
                }
            })
            .collect()
    }
}

/// One message queue, as `/proc/sysvipc/msg` shows it.
#[allow(dead_code, reason = "read by procfs")]
pub(crate) struct MsgInfo {
    pub(crate) id: i32,
    pub(crate) perm: Perm,
    pub(crate) cbytes: usize,
    pub(crate) qnum: usize,
    pub(crate) lspid: u32,
    pub(crate) lrpid: u32,
    pub(crate) stime: u64,
    pub(crate) rtime: u64,
    pub(crate) ctime: u64,
}

impl Kernel {
    pub(crate) fn sys_msgget(&self, caller: &Caller<'_>, key: i32, msgflg: i32) -> SysResult<i32> {
        let cred = self.env(caller).cred;
        let now = self.ipc_now();
        let create = |perm| {
            Ok(MsgQueue {
                state: Mutex::new(State {
                    removed: false,
                    messages: VecDeque::new(),
                    cbytes: 0,
                    qbytes: MSGMNB as usize,
                    lspid: 0,
                    lrpid: 0,
                    stime: 0,
                    rtime: 0,
                    ctime: now,
                }),
                perm: Mutex::new(perm),
                queue: WaitQueue::new(),
            })
        };
        let ids = &self.ipc.msg.ids;
        let limit = MSGMNI as usize;
        Ok(get_or_create(
            ids,
            key,
            msgflg,
            &cred,
            limit,
            |_| Ok(()),
            create,
        )?)
    }

    pub(crate) async fn sys_msgsnd(
        &self,
        caller: &Caller<'_>,
        msqid: i32,
        msgp: ConstMsgbufPtr,
        msgsz: u32,
        msgflg: i32,
    ) -> SysResult<i32> {
        let mtype: i32 = read_value(caller.memory(), GuestPtr::new(msgp.addr())).await?;
        if msgsz > MSGMAX as u32 || msqid < 0 || mtype < 1 {
            return Err(Errno::EINVAL.into());
        }
        let mut text = vec![0; msgsz as usize];
        caller.memory().read(mtext(msgp.addr())?, &mut text).await?;
        let cred = self.env(caller).cred;
        let pid = self.process(caller.task()).pid.0;
        let queue = self.ipc.msg.get(msqid)?;
        let nowait = IpcFlags(msgflg as u32).contains(IpcFlags::IPC_NOWAIT);
        let mut message = Some(Message { mtype, text });
        let send = || -> Option<Result<(), Errno>> {
            let mut state = queue.state();
            if let Err(errno) = queue.perm().check(&cred, WRITE) {
                return Some(Err(errno));
            }
            if state.removed {
                return Some(Err(Errno::EIDRM));
            }
            let size = message.as_ref().expect("sent once").text.len();
            if !state.fits(size) {
                return nowait.then_some(Err(Errno::EAGAIN));
            }
            state.lspid = pid;
            state.stime = self.ipc_now();
            state.cbytes += size;
            state.messages.push_back(message.take().expect("sent once"));
            drop(state);
            queue.queue.wake(PollMask::empty());
            Some(Ok(()))
        };
        let interrupt = self.interrupt(caller);
        match wait_event(&queue.queue, send, &*interrupt).await {
            Ok(result) => Ok(result.map(|()| 0)?),
            Err(_) => Err(SysErr::RestartNoHand),
        }
    }

    pub(crate) async fn sys_msgrcv(
        &self,
        caller: &Caller<'_>,
        msqid: i32,
        msgp: MsgbufPtr,
        msgsz: u32,
        msgtyp: i32,
        msgflg: i32,
    ) -> SysResult<i32> {
        if msqid < 0 || (msgsz as i32) < 0 {
            return Err(Errno::EINVAL.into());
        }
        let flags = MsgrcvFlags(msgflg as u32);
        let nowait = IpcFlags(msgflg as u32).contains(IpcFlags::IPC_NOWAIT);
        let copy = flags.contains(MsgrcvFlags::MSG_COPY);
        if copy && (flags.contains(MsgrcvFlags::MSG_EXCEPT) || !nowait) {
            return Err(Errno::EINVAL.into());
        }
        let search = match msgtyp {
            n if copy => Search::Number(n),
            0 => Search::Any,
            i32::MIN => Search::LessEqual(i32::MAX),
            n if n < 0 => Search::LessEqual(-n),
            n if flags.contains(MsgrcvFlags::MSG_EXCEPT) => Search::NotEqual(n),
            n => Search::Equal(n),
        };
        let noerror = flags.contains(MsgrcvFlags::MSG_NOERROR);
        let cred = self.env(caller).cred;
        let pid = self.process(caller.task()).pid.0;
        let queue = self.ipc.msg.get(msqid)?;
        let receive = || -> Option<Result<Message, Errno>> {
            let mut state = queue.state();
            if let Err(errno) = queue.perm().check(&cred, READ) {
                return Some(Err(errno));
            }
            if state.removed {
                return Some(Err(Errno::EIDRM));
            }
            let Some(at) = state.find(search) else {
                return nowait.then_some(Err(Errno::ENOMSG));
            };
            if (msgsz as usize) < state.messages[at].text.len() && !noerror {
                return Some(Err(Errno::E2BIG));
            }
            if copy {
                let message = &state.messages[at];
                return Some(Ok(Message {
                    mtype: message.mtype,
                    text: message.text.clone(),
                }));
            }
            let message = state.messages.remove(at).expect("found above");
            state.cbytes -= message.text.len();
            state.rtime = self.ipc_now();
            state.lrpid = pid;
            drop(state);
            queue.queue.wake(PollMask::empty());
            Some(Ok(message))
        };
        let interrupt = self.interrupt(caller);
        let message = match wait_event(&queue.queue, receive, &*interrupt).await {
            Ok(result) => result?,
            Err(_) => return Err(SysErr::RestartNoHand),
        };
        let n = message.text.len().min(msgsz as usize);
        write_value(
            caller.memory(),
            GuestPtrMut::new(msgp.addr()),
            &message.mtype,
        )
        .await?;
        caller
            .memory()
            .write(mtext(msgp.addr())?, &message.text[..n])
            .await?;
        Ok(n as i32)
    }

    pub(crate) async fn sys_msgctl(
        &self,
        caller: &Caller<'_>,
        msqid: i32,
        cmd: i32,
        buf: u32,
    ) -> SysResult<i32> {
        if msqid < 0 {
            return Err(Errno::EINVAL.into());
        }
        let cred = self.env(caller).cred;
        let queues = &self.ipc.msg;
        match IpcCmd(cmd) {
            IpcCmd::IPC_INFO | IpcCmd::MSG_INFO => {
                let ((pool, map, tql), max) = {
                    let ids = lock(&queues.ids);
                    let totals = match IpcCmd(cmd) == IpcCmd::MSG_INFO {
                        true => {
                            let all = ids.all();
                            let (msgs, bytes) = all.iter().fold((0, 0), |(n, b), (_, q)| {
                                let state = q.state();
                                (n + state.messages.len(), b + state.cbytes)
                            });
                            (ids.in_use() as i32, msgs as i32, bytes as i32)
                        }
                        false => (MSGPOOL, MSGMAP, MSGTQL),
                    };
                    (totals, ids.max_index())
                };
                let ints = [pool, map, MSGMAX, MSGMNB, MSGMNI, MSGSSZ, tql];
                let mut bytes: Vec<u8> = ints.iter().flat_map(|v| v.to_le_bytes()).collect();
                bytes.extend(MSGSEG.to_le_bytes());
                bytes.extend([0, 0]);
                caller.memory().write(buf, &bytes).await?;
                Ok(max)
            }
            IpcCmd::IPC_STAT | IpcCmd::MSG_STAT | super::MSG_STAT_ANY => {
                let (id, queue) = match IpcCmd(cmd) {
                    IpcCmd::IPC_STAT => (msqid, queues.get(msqid)?),
                    _ => {
                        let queue = lock(&queues.ids).at(msqid).ok_or(Errno::EINVAL)?;
                        let id = super::build_id(msqid as usize, queue.perm().seq);
                        (id, queue)
                    }
                };
                let ds = {
                    let state = queue.state();
                    let perm = queue.perm();
                    if IpcCmd(cmd) != super::MSG_STAT_ANY {
                        perm.check(&cred, READ)?;
                    }
                    state.to_abi(&perm)
                };
                write_value(caller.memory(), GuestPtrMut::new(buf), &ds).await?;
                Ok(match IpcCmd(cmd) {
                    IpcCmd::IPC_STAT => 0,
                    _ => id,
                })
            }
            IpcCmd::IPC_SET => {
                let ds: Msqid64Ds = read_value(caller.memory(), GuestPtr::new(buf)).await?;
                let queue = queues.get(msqid)?;
                let mut state = queue.state();
                let mut perm = queue.perm();
                perm.check_owner(&cred)?;
                let qbytes = ds.msg_qbytes as usize;
                if qbytes > MSGMNB as usize && !capable(&cred, CAP_SYS_RESOURCE) {
                    return Err(Errno::EPERM.into());
                }
                state.qbytes = qbytes;
                perm.update(&ds.msg_perm);
                state.ctime = self.ipc_now();
                drop((perm, state));
                queue.queue.wake(PollMask::empty());
                Ok(0)
            }
            IpcCmd::IPC_RMID => {
                let mut ids = lock(&queues.ids);
                let queue = ids.get(msqid).ok_or(Errno::EINVAL)?;
                queue.perm().check_owner(&cred)?;
                ids.remove(msqid);
                drop(ids);
                // `freeque`: senders and receivers fail with EIDRM.
                let mut state = queue.state();
                state.removed = true;
                state.messages.clear();
                state.cbytes = 0;
                drop(state);
                queue.queue.wake(PollMask::empty());
                Ok(0)
            }
            _ => Err(Errno::EINVAL.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_buffer_at_the_top_of_memory_faults() {
        assert_eq!(mtext(0x100), Ok(0x104));
        assert_eq!(mtext(u32::MAX - 3), Err(Errno::EFAULT));
    }
}
