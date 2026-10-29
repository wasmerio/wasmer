//! Request transport: fs/fuse/dev.c, fuse_dev_do_read/write and request_wait_answer.
use super::{Connection, wire::*};
use crate::{
    file::{FileObject, Io},
    ns::UserNs,
    sync::MutexExt,
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use wasixv2_abi::IoctlRequest;
use wasixv2_syscalls::{Caller, Errno, LocalFuture, SysResult};
use wasixv2_wait::{NoInterrupt, PollMask, PollTable, Pollable, WaitQueue, poll_wait, wait_event};

pub(super) enum Completion {
    Reply,
    Poll(Weak<super::poll::PollState>, u64),
    Forget(u64),
}

struct Waiting<'a> {
    conn: &'a Connection,
    req: Arc<Request>,
    done: bool,
}
impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        if !self.done {
            self.conn.cancel_request(&self.req);
        }
    }
}

pub(super) struct Request {
    pub(super) unique: u64,
    pub(super) opcode: u32,
    bytes: Vec<u8>,
    device: AtomicU64,
    completion: Completion,
    abandoned: AtomicBool,
    _inode: Option<Arc<super::fs::FuseNode>>,
    pub(super) reply: Mutex<Option<Result<Vec<u8>, Errno>>>,
    pub(super) wait: WaitQueue,
    /// Inclusive payload size range; fuse_copy_out_args validates it before completing.
    min: usize,
    max: usize,
}

pub(super) struct Queue {
    pub(super) connected: bool,
    pub(super) initialized: bool,
    pub(super) init_error: bool,
    pub(super) destroy_sent: bool,
    pub(super) minor: u32,
    pub(super) max_write: usize,
    pub(super) flags: u32,
    pub(super) max_background: u16,
    pub(super) congestion_threshold: u16,
    next: u64,
    pending: VecDeque<Arc<Request>>,
    processing: BTreeMap<u64, Arc<Request>>,
    forgets: BTreeMap<u64, u64>,
    no_interrupt: bool,
    interrupted: BTreeSet<u64>,
    interrupts: VecDeque<u64>,
}
impl Default for Queue {
    fn default() -> Self {
        Self {
            connected: true,
            initialized: false,
            init_error: false,
            destroy_sent: false,
            minor: 0,
            max_write: 4096,
            max_background: 12,
            congestion_threshold: 9,
            flags: 0,
            next: 0,
            pending: VecDeque::new(),
            processing: BTreeMap::new(),
            forgets: BTreeMap::new(),
            no_interrupt: false,
            interrupted: BTreeSet::new(),
            interrupts: VecDeque::new(),
        }
    }
}

impl Queue {
    pub(super) fn waiting(&self) -> usize {
        self.pending.len() + self.processing.len()
    }
}

impl Connection {
    /// fs/fuse/dev.c:fuse_request_assign_unique. Bit zero is reserved for INTERRUPT.
    pub(super) fn enqueue(
        &self,
        opcode: u32,
        node: u64,
        body: Vec<u8>,
        min: usize,
        max: usize,
    ) -> Arc<Request> {
        self.enqueue_completion(opcode, node, body, min, max, Completion::Reply)
    }
    pub(super) fn enqueue_completion(
        &self,
        opcode: u32,
        node: u64,
        body: Vec<u8>,
        min: usize,
        max: usize,
        completion: Completion,
    ) -> Arc<Request> {
        let (uid, gid, pid, _) = if opcode == DESTROY {
            (0, 0, 0, None)
        } else {
            self.current()
        };
        let inode = self
            .nodes
            .lock_or_recover()
            .get(&node)
            .and_then(Weak::upgrade);
        let (req, queued) = {
            let mut q = self.queue.lock_or_recover();
            q.next += 2;
            let unique = q.next;
            let mut bytes = vec![0; HEADER];
            put32(&mut bytes, 0, (HEADER + body.len()) as u32);
            put32(&mut bytes, 4, opcode);
            put64(&mut bytes, 8, unique);
            put64(&mut bytes, 16, node);
            put32(&mut bytes, 24, uid);
            put32(&mut bytes, 28, gid);
            put32(&mut bytes, 32, pid);
            bytes.extend(body);
            let req = Arc::new(Request {
                unique,
                opcode,
                bytes,
                device: AtomicU64::new(0),
                completion,
                abandoned: AtomicBool::new(false),
                _inode: inode,
                reply: Mutex::new(None),
                wait: WaitQueue::new(),
                min,
                max,
            });
            if q.connected {
                q.pending.push_back(req.clone());
            }
            (req, q.connected)
        };
        if !queued {
            self.finish(&req, Err(Errno::ENOTCONN));
        }
        self.readable.wake(PollMask::IN);
        req
    }

    pub(super) async fn ready(&self) -> Result<(), Errno> {
        let (_, _, _, signals) = self.current();
        let no = NoInterrupt;
        let interrupt: &dyn wasixv2_wait::Interrupt = signals
            .as_ref()
            .map_or(&no, |s| &**s as &dyn wasixv2_wait::Interrupt);
        wait_event(
            &self.changed,
            || {
                let q = self.queue.lock_or_recover();
                if !q.connected || q.destroy_sent {
                    Some(Err(Errno::ENOTCONN))
                } else if q.init_error {
                    Some(Err(Errno::ECONNREFUSED))
                } else if q.initialized {
                    Some(Ok(()))
                } else {
                    None
                }
            },
            interrupt,
        )
        .await
        .map_err(|_| Errno::EINTR)?
    }

    pub(super) async fn request(
        &self,
        opcode: u32,
        node: u64,
        body: Vec<u8>,
        min: usize,
        max: usize,
    ) -> Result<Vec<u8>, Errno> {
        self.ready().await?;
        let req = self.enqueue(opcode, node, body, min, max);
        let mut waiting = Waiting {
            conn: self,
            req: req.clone(),
            done: false,
        };
        let (_, _, _, signals) = self.current();
        let no = NoInterrupt;
        let interrupt: &dyn wasixv2_wait::Interrupt = signals
            .as_ref()
            .map_or(&no, |s| &**s as &dyn wasixv2_wait::Interrupt);
        let answer = || req.reply.lock_or_recover().take();
        // request_wait_answer sends INTERRUPT, then waits for the original reply. Never
        // restart an already-delivered mutation merely because a signal arrived.
        if !self.queue.lock_or_recover().no_interrupt {
            if let Ok(result) = wait_event(&req.wait, answer, interrupt).await {
                waiting.done = true;
                return result;
            }
            let mut q = self.queue.lock_or_recover();
            q.interrupted.insert(req.unique);
            if q.processing.contains_key(&req.unique) {
                q.interrupts.push_back(req.unique);
            }
            drop(q);
            self.readable.wake(PollMask::READABLE);
        }
        if let Some(signals) = &signals {
            if let Ok(result) = wait_event(&req.wait, answer, &signals.killable()).await {
                waiting.done = true;
                return result;
            }
            // A fatal signal may cancel a request only before userspace has received it.
            let mut q = self.queue.lock_or_recover();
            if let Some(at) = q.pending.iter().position(|r| r.unique == req.unique) {
                q.pending.remove(at);
                q.interrupted.remove(&req.unique);
                return Err(Errno::EINTR);
            }
            // The daemon owns a delivered request. Fatal teardown leaves it interrupted
            // without a waiter (request_wait_answer); the eventual reply retires it.
            return Err(Errno::EINTR);
        }
        let result = wait_event(&req.wait, answer, &NoInterrupt).await.unwrap();
        waiting.done = true;
        result
    }

    fn cancel_request(&self, req: &Request) {
        req.abandoned.store(true, Ordering::Release);
        let reply = req.reply.lock_or_recover().take();
        if let Some(Ok(body)) = reply {
            self.discard_reply(req, &body);
        }
        let mut q = self.queue.lock_or_recover();
        if let Some(at) = q.pending.iter().position(|r| r.unique == req.unique) {
            q.pending.remove(at);
            q.interrupted.remove(&req.unique);
            q.interrupts.retain(|id| *id != req.unique);
        } else if q.processing.contains_key(&req.unique)
            && !q.no_interrupt
            && q.interrupted.insert(req.unique)
        {
            q.interrupts.push_back(req.unique);
        }
        drop(q);
        self.readable.wake(PollMask::READABLE);
    }

    fn discard_reply(&self, req: &Request, body: &[u8]) {
        // fuse_lookup_name/fuse_atomic_open retire entry and handle references when
        // a delivered operation cannot install its inode or file.
        let mut entry = None;
        let mut opened = None;
        match req.opcode {
            1 | 6 | 8 | 9 | 13 => entry = u64_at(body, 0).ok(),
            35 => {
                entry = u64_at(body, 0).ok();
                opened = u64_at(body, 128)
                    .ok()
                    .map(|fh| (18, entry.unwrap_or(0), fh));
            }
            14 | 27 => {
                opened = u64_at(body, 0).ok().map(|fh| {
                    (
                        if req.opcode == 27 { 29 } else { 18 },
                        u64_at(&req.bytes, 16).unwrap_or(0),
                        fh,
                    )
                })
            }
            44 => {
                let mut at = 0;
                while body.len() - at >= 152 {
                    let len = u32_at(body, at + 144).unwrap_or(0) as usize;
                    let size = (152 + len + 7) & !7;
                    if len == 0 || size > body.len() - at {
                        break;
                    }
                    let name = &body[at + 152..at + 152 + len];
                    if name != b"."
                        && name != b".."
                        && let Ok(id) = u64_at(body, at)
                        && id > 1
                    {
                        self.forget(id, 1);
                    }
                    at += size;
                }
            }
            _ => {}
        }
        if let Some((opcode, id, fh)) = opened {
            let mut input = words64(&[fh]);
            input.extend(words32(&[u32_at(&req.bytes, HEADER).unwrap_or(0), 0]));
            input.extend(words64(&[0]));
            let completion = entry.take().map_or(Completion::Reply, Completion::Forget);
            self.enqueue_completion(opcode, id, input, 0, 0, completion);
        }
        if let Some(id) = entry
            && id > 1
        {
            self.forget(id, 1);
        }
    }

    pub(super) fn destroy(&self) -> Option<Arc<Request>> {
        let mut q = self.queue.lock_or_recover();
        if q.destroy_sent {
            return None;
        }
        q.destroy_sent = true;
        let send = q.connected && !q.init_error;
        drop(q);
        if send {
            Some(self.enqueue(DESTROY, 0, Vec::new(), 0, 0))
        } else {
            self.remove_control();
            None
        }
    }

    pub(super) fn detached_flush(&self, node: u64, body: Vec<u8>) {
        // fs/fuse/file.c:fuse_flush; dev.c:request_wait_answer. The fatal caller cannot
        // sleep. Keep the background exchange owned by the connection, not its task.
        if self.unsupported.lock_or_recover().contains(&25) {
            return;
        }
        let req = self.enqueue(25, node, body, 0, 0);
        let mut q = self.queue.lock_or_recover();
        q.interrupted.insert(req.unique);
        if q.processing.contains_key(&req.unique) {
            q.interrupts.push_back(req.unique);
        }
        drop(q);
        self.readable.wake(PollMask::READABLE);
    }

    fn finish(&self, req: &Request, result: Result<Vec<u8>, Errno>) {
        if req.opcode == DESTROY {
            self.abort();
            self.remove_control();
        }
        if req.opcode == INIT {
            self.finish_init(result.clone());
        }
        if let Completion::Poll(poll, epoch) = &req.completion
            && let Some(poll) = poll.upgrade()
        {
            let mask = match &result {
                Ok(body) => u32_at(body, 0).unwrap_or(PollMask::ERR.0),
                Err(Errno::ENOSYS) => {
                    self.unsupported.lock_or_recover().insert(40);
                    PollMask::DEFAULT.0
                }
                Err(_) => PollMask::ERR.0,
            };
            poll.complete(*epoch, mask);
        }
        if let Completion::Forget(id) = req.completion
            && id > 1
        {
            self.forget(id, 1);
        }
        let mut slot = req.reply.lock_or_recover();
        if req.abandoned.load(Ordering::Acquire) {
            drop(slot);
            if let Ok(body) = &result {
                self.discard_reply(req, body);
            }
        } else {
            *slot = Some(result);
            drop(slot);
        }
        req.wait.wake(PollMask::IN);
    }

    fn release_device(&self, device: u64) {
        let requests = {
            let mut q = self.queue.lock_or_recover();
            let ids: Vec<_> = q
                .processing
                .iter()
                .filter(|(_, r)| r.device.load(Ordering::Acquire) == device)
                .map(|(&id, _)| id)
                .collect();
            let requests: Vec<_> = ids
                .iter()
                .filter_map(|id| q.processing.remove(id))
                .collect();
            for id in ids {
                q.interrupted.remove(&id);
                q.interrupts.retain(|n| *n != id);
            }
            requests
        };
        for req in requests {
            self.finish(&req, Err(Errno::ECONNABORTED));
        }
    }

    pub(super) fn forget(&self, node: u64, nlookup: u64) {
        let mut q = self.queue.lock_or_recover();
        if q.connected {
            *q.forgets.entry(node).or_default() += nlookup;
        }
        drop(q);
        self.readable.wake(PollMask::IN);
    }

    pub(super) fn abort(&self) {
        let requests = {
            let mut q = self.queue.lock_or_recover();
            q.connected = false;
            q.forgets.clear();
            let mut requests: Vec<_> = q.pending.drain(..).collect();
            requests.extend(std::mem::take(&mut q.processing).into_values());
            requests
        };
        for req in requests {
            self.finish(&req, Err(Errno::ECONNABORTED));
        }
        let polls: Vec<_> = self
            .polls
            .lock_or_recover()
            .values()
            .filter_map(std::sync::Weak::upgrade)
            .collect();
        for poll in polls {
            poll.ready.store(PollMask::ERR.0, Ordering::Release);
            poll.notify();
        }
        self.changed.wake(PollMask::ERR);
        self.readable.wake(PollMask::ERR);
    }

    fn read_request(&self, device: u64, buf: &mut [u8]) -> Option<Result<usize, Errno>> {
        let mut q = self.queue.lock_or_recover();
        if !q.connected {
            return Some(Err(Errno::ENODEV));
        }
        if buf.len() < MIN_READ.max(HEADER + 40 + q.max_write) {
            return Some(Err(Errno::EINVAL));
        }
        while let Some(unique) = q.interrupts.pop_front() {
            if !q.processing.contains_key(&unique) {
                continue;
            }
            buf[..HEADER + 8].fill(0);
            put32(buf, 0, (HEADER + 8) as u32);
            put32(buf, 4, INTERRUPT);
            put64(buf, 8, unique | 1);
            put64(buf, HEADER, unique);
            return Some(Ok(HEADER + 8));
        }
        // fuse_read_batch_forget: all forgets fit as many records as this read can hold.
        if !q.forgets.is_empty() {
            let batch = q.minor >= 16;
            let count = if batch {
                ((buf.len() - HEADER - 8) / 16).min(q.forgets.len())
            } else {
                1
            };
            let mut body = if batch {
                words32(&[count as u32, 0])
            } else {
                Vec::new()
            };
            let mut nodeid = 0;
            for _ in 0..count {
                let (node, nlookup) = q.forgets.pop_first().unwrap();
                nodeid = node;
                if batch {
                    body.extend(words64(&[node]));
                }
                body.extend(words64(&[nlookup]));
            }
            q.next += 2;
            buf[..HEADER].fill(0);
            put32(buf, 0, (HEADER + body.len()) as u32);
            put32(buf, 4, if batch { BATCH_FORGET } else { FORGET });
            put64(buf, 8, q.next);
            put64(buf, 16, if batch { 0 } else { nodeid });
            buf[HEADER..HEADER + body.len()].copy_from_slice(&body);
            return Some(Ok(HEADER + body.len()));
        }
        loop {
            let req = q.pending.pop_front()?;
            let len = req.bytes.len();
            if len > buf.len() {
                // fuse_dev_do_read restarts after retiring an oversized request. SETXATTR
                // is special because its caller may retry with a smaller value.
                drop(q);
                self.finish(
                    &req,
                    Err(if req.opcode == 21 {
                        Errno::E2BIG
                    } else {
                        Errno::EIO
                    }),
                );
                drop(req);
                q = self.queue.lock_or_recover();
                continue;
            }
            buf[..len].copy_from_slice(&req.bytes);
            req.device.store(device, Ordering::Release);
            if q.interrupted.contains(&req.unique) {
                q.interrupts.push_back(req.unique);
            }
            q.processing.insert(req.unique, req);
            return Some(Ok(len));
        }
    }

    fn write_reply(&self, device: u64, data: &[u8]) -> Result<usize, Errno> {
        if data.len() < OUT_HEADER || u32_at(data, 0)? as usize != data.len() {
            return Err(Errno::EINVAL);
        }
        let error = u32_at(data, 4)? as i32;
        let unique = u64_at(data, 8)?;
        if unique == 0 {
            let q = self.queue.lock_or_recover();
            if !q.initialized || !q.connected || q.init_error {
                return Err(Errno::EINVAL);
            }
            drop(q);
            self.notify(error, &data[OUT_HEADER..])?;
            return Ok(data.len());
        }
        if error > 0 || error <= -512 {
            return Err(Errno::EINVAL);
        }
        let req = {
            let mut q = self.queue.lock_or_recover();
            if !q.connected
                || q.processing
                    .get(&(unique & !1))
                    .is_none_or(|r| r.device.load(Ordering::Acquire) != device)
            {
                return Err(Errno::ENOENT);
            }
            if unique & 1 != 0 {
                if !q.processing.contains_key(&(unique & !1)) {
                    return Err(Errno::ENOENT);
                }
                if data.len() != OUT_HEADER {
                    return Err(Errno::EINVAL);
                }
                if error == -Errno::ENOSYS.0 {
                    q.no_interrupt = true;
                }
                if error == -Errno::EAGAIN.0 {
                    q.interrupts.push_back(unique & !1);
                }
                drop(q);
                self.readable.wake(PollMask::READABLE);
                return Ok(data.len());
            }
            q.interrupted.remove(&unique);
            q.interrupts.retain(|id| *id != unique);
            q.processing.remove(&unique).ok_or(Errno::ENOENT)?
        };
        let body = &data[OUT_HEADER..];
        let valid = if error != 0 {
            body.is_empty()
        } else {
            body.len() >= req.min && body.len() <= req.max
        };
        let result = if !valid {
            Err(Errno::EIO)
        } else if error != 0 {
            Err(Errno(-error))
        } else {
            Ok(body.to_vec())
        };
        self.finish(&req, result);
        if !valid {
            Err(Errno::EINVAL)
        } else {
            Ok(data.len())
        }
    }

    fn finish_init(&self, result: Result<Vec<u8>, Errno>) {
        if let Ok(body) = &result
            && u32_at(body, 0).is_ok_and(|major| major > 7)
        {
            let mut init = vec![0; 64];
            put32(&mut init, 0, 7);
            put32(&mut init, 4, MINOR);
            put32(&mut init, 12, super::INIT_FLAGS | (1 << 30));
            self.enqueue(INIT, 0, init, 8, 64);
            return;
        }
        let mut q = self.queue.lock_or_recover();
        match result {
            Ok(body)
                if u32_at(&body, 0) == Ok(7) && u32_at(&body, 4).is_ok_and(|minor| minor >= 23) =>
            {
                q.minor = u32_at(&body, 4).unwrap().min(MINOR);
                q.flags = u32_at(&body, 12).unwrap_or(0) & super::INIT_FLAGS;
                q.max_write =
                    (u32_at(&body, 20).unwrap_or(4096) as usize).clamp(4096, MAX_IO - HEADER - 40);
            }
            _ => q.init_error = true,
        }
        q.initialized = true;
        drop(q);
        self.changed.wake(PollMask::IN);
    }
}

/// A device is unbound until fuse_get_tree attaches a connection; clones share its queues.
pub(crate) struct Device {
    pub(super) id: u64,
    pub(super) conn: Mutex<Option<Arc<Connection>>>,
    pub(super) user_ns: Arc<UserNs>,
    pub(super) sync_init: AtomicBool,
}
impl Device {
    fn connection(&self) -> Result<Arc<Connection>, Errno> {
        self.conn.lock_or_recover().clone().ok_or(Errno::EPERM)
    }
}
impl Drop for Device {
    fn drop(&mut self) {
        if let Some(conn) = self
            .conn
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            conn.release_device(self.id);
            if conn.devices.fetch_sub(1, Ordering::AcqRel) == 1 {
                conn.abort();
            }
        }
    }
}
impl Pollable for Device {
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        let Ok(conn) = self.connection() else {
            return PollMask::ERR;
        };
        poll_wait(&mut table, &conn.readable);
        let q = conn.queue.lock_or_recover();
        if !q.connected {
            PollMask::ERR
        } else if !q.pending.is_empty() || !q.forgets.is_empty() || !q.interrupts.is_empty() {
            PollMask::READABLE | PollMask::WRITABLE
        } else {
            PollMask::WRITABLE
        }
    }
}
impl FileObject for Device {
    fn read_size(&self) -> usize {
        MAX_IO + HEADER + 16 + 256
    }
    fn read_to<'a>(
        &'a self,
        io: Io<'a>,
        buf: &'a mut [u8],
        to: &'a mut dyn crate::file::ReadSink,
    ) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move {
            let n = self.read(io, buf).await?;
            if to.copy_out(&buf[..n]).await != n {
                let unique = u64_at(buf, 8)?;
                let conn = self.connection()?;
                let req = { conn.queue.lock_or_recover().processing.remove(&unique) };
                if let Some(req) = req {
                    conn.finish(&req, Err(Errno::EIO));
                }
                return Err(Errno::EFAULT.into());
            }
            Ok(n)
        })
    }

    fn read<'a>(&'a self, io: Io<'a>, buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move {
            let conn = self.connection()?;
            if io.nonblocking() {
                return conn
                    .read_request(self.id, buf)
                    .unwrap_or(Err(Errno::EAGAIN))
                    .map_err(Into::into);
            }
            wait_event(
                &conn.readable,
                || conn.read_request(self.id, buf),
                io.interrupt,
            )
            .await
            .map_err(|_| wasixv2_syscalls::SysErr::Restart)?
            .map_err(Into::into)
        })
    }
    fn write<'a>(&'a self, _: Io<'a>, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move {
            self.connection()?
                .write_reply(self.id, data)
                .map_err(Into::into)
        })
    }
    fn messages(&self) -> bool {
        true
    }
    fn ioctl<'a>(
        &'a self,
        caller: &'a Caller<'a>,
        request: IoctlRequest,
        arg: u32,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async move {
            match request {
                IoctlRequest::FUSE_DEV_IOC_SYNC_INIT => {
                    let bound = self.conn.lock_or_recover();
                    if bound.is_some() {
                        return Err(Errno::EINVAL.into());
                    }
                    self.sync_init.store(true, Ordering::Release);
                    Ok(0)
                }
                _ => {
                    let _ = (caller, arg);
                    Err(Errno::ENOTTY.into())
                }
            }
        })
    }
}
