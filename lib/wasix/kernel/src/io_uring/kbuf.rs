//! Provided buffers (`io_uring/kbuf.c`): legacy lists and retained user/MMAP rings.

use super::Ctx;
use crate::{
    Kernel,
    mm::{PAGE_SIZE, Retained},
    sync::MutexExt,
};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicU16, Ordering},
    },
};
use wasixv2_abi::{
    GuestLayout, GuestPtr, GuestPtrMut, IoUringBuf, IoUringBufReg, IoUringBufStatus, IoUringSqe,
    IoringCqeFlags,
};
use wasixv2_pages::SharedPages;
use wasixv2_syscalls::{Caller, Errno, SysResult, read_value, write_value};

pub(super) enum Group {
    Legacy(VecDeque<Pbuf>),
    Ring(Arc<BufferRing>),
}
impl Default for Group {
    fn default() -> Self {
        Self::Legacy(VecDeque::new())
    }
}

pub(super) struct BufferRing {
    storage: Storage,
    mask: u16,
    head: AtomicU16,
    inc: bool,
    min_left: u32,
}

enum Storage {
    User(Arc<Retained>),
    Mapped(Arc<dyn SharedPages>),
}

#[derive(Clone)]
pub(crate) struct Pbuf {
    pub(crate) addr: u32,
    pub(crate) len: u32,
    pub(crate) bid: u16,
    ring: Option<Arc<BufferRing>>,
    full_len: u32,
}

impl BufferRing {
    async fn read(&self, caller: &Caller<'_>, at: u32, out: &mut [u8]) -> Result<(), Errno> {
        match &self.storage {
            Storage::User(range) => range
                .read(caller.memory(), at, out)
                .await
                .map_err(|_| Errno::EFAULT),
            Storage::Mapped(pages) => pages.read_at(out, u64::from(at)).map_err(|_| Errno::EFAULT),
        }
    }
    async fn write(&self, caller: &Caller<'_>, at: u32, data: &[u8]) -> Result<(), Errno> {
        match &self.storage {
            Storage::User(range) => range
                .write(caller.memory(), at, data)
                .await
                .map_err(|_| Errno::EFAULT),
            Storage::Mapped(pages) => pages
                .write_at(data, u64::from(at))
                .map_err(|_| Errno::EFAULT),
        }
    }
}

impl Kernel {
    pub(super) async fn register_pbuf_ring(
        &self,
        caller: &Caller<'_>,
        ctx: &Ctx,
        arg: u32,
    ) -> SysResult<i32> {
        let reg: IoUringBufReg = read_value(caller.memory(), GuestPtr::new(arg)).await?;
        if reg.resv != [0; 5]
            || reg.flags & !3 != 0
            || !reg.ring_entries.is_power_of_two()
            || reg.ring_entries >= 65536
            || (reg.flags & 2 == 0 && reg.min_left != 0)
        {
            return Err(Errno::EINVAL.into());
        }
        {
            let state = ctx.state.lock_or_recover();
            if let Some(group) = state.pbufs.get(&reg.bgid)
                && !matches!(group, Group::Legacy(list) if list.is_empty())
            {
                return Err(Errno::EEXIST.into());
            }
        }
        let size = (u64::from(reg.ring_entries) * 16).div_ceil(PAGE_SIZE) * PAGE_SIZE;
        let storage = if reg.flags & 1 != 0 {
            Storage::Mapped(self.host.shared_pages(size).map_err(|_| Errno::ENOMEM)?)
        } else {
            if reg.ring_addr == 0 {
                return Err(Errno::EFAULT.into());
            }
            if !reg.ring_addr.is_multiple_of(PAGE_SIZE) {
                return Err(Errno::EINVAL.into());
            }
            let addr = u32::try_from(reg.ring_addr).map_err(|_| Errno::EFAULT)?;
            Storage::User(self.retain_pages(caller, addr, size as u32).await?)
        };
        let ring = BufferRing {
            storage,
            mask: (reg.ring_entries - 1) as u16,
            head: AtomicU16::new(0),
            inc: reg.flags & 2 != 0,
            min_left: reg.min_left.max(1),
        };
        ctx.state
            .lock_or_recover()
            .pbufs
            .insert(reg.bgid, Group::Ring(Arc::new(ring)));
        Ok(0)
    }

    pub(super) async fn unregister_pbuf_ring(
        &self,
        caller: &Caller<'_>,
        ctx: &Ctx,
        arg: u32,
    ) -> SysResult<i32> {
        let reg: IoUringBufReg = read_value(caller.memory(), GuestPtr::new(arg)).await?;
        if reg.resv != [0; 5] || reg.flags != 0 {
            return Err(Errno::EINVAL.into());
        }
        let old = {
            let mut state = ctx.state.lock_or_recover();
            match state.pbufs.get(&reg.bgid).ok_or(Errno::ENOENT)? {
                Group::Legacy(_) => return Err(Errno::EINVAL.into()),
                Group::Ring(_) => state.pbufs.remove(&reg.bgid),
            }
        };
        drop(old);
        Ok(0)
    }

    pub(super) async fn pbuf_status(
        &self,
        caller: &Caller<'_>,
        ctx: &Ctx,
        arg: u32,
    ) -> SysResult<i32> {
        let mut status: IoUringBufStatus = read_value(caller.memory(), GuestPtr::new(arg)).await?;
        if status.resv != [0; 8] {
            return Err(Errno::EINVAL.into());
        }
        status.head = {
            let state = ctx.state.lock_or_recover();
            let id = u16::try_from(status.buf_group).map_err(|_| Errno::ENOENT)?;
            match state.pbufs.get(&id).ok_or(Errno::ENOENT)? {
                Group::Legacy(_) => return Err(Errno::EINVAL.into()),
                Group::Ring(ring) => u32::from(ring.head.load(Ordering::Relaxed)),
            }
        };
        write_value(caller.memory(), GuestPtrMut::new(arg), &status).await?;
        Ok(0)
    }
}

pub(super) fn mmap(ctx: &Ctx, offset: u64) -> Result<Arc<dyn SharedPages>, Errno> {
    let id = ((offset & !0xf800_0000) >> 16) as u16;
    let state = ctx.state.lock_or_recover();
    if let Some(Group::Ring(ring)) = state.pbufs.get(&id)
        && let Storage::Mapped(pages) = &ring.storage
    {
        return Ok(pages.clone());
    }
    Err(Errno::EINVAL)
}

/// `io_buffer_select`, serialized with commits/registration by ctx.submit.
pub(super) async fn select(
    caller: &Caller<'_>,
    ctx: &Ctx,
    mm: u64,
    bgid: u16,
    len: u32,
) -> Result<Pbuf, Errno> {
    let ring = {
        let mut state = ctx.state.lock_or_recover();
        match state.pbufs.get_mut(&bgid).ok_or(Errno::ENOBUFS)? {
            Group::Legacy(list) => {
                let mut buffer = list.pop_front().ok_or(Errno::ENOBUFS)?;
                if len != 0 {
                    buffer.len = buffer.len.min(len);
                }
                return Ok(buffer);
            }
            Group::Ring(ring) => ring.clone(),
        }
    };
    if let Storage::User(range) = &ring.storage {
        range.check_mm(mm)?;
    }
    let head = ring.head.load(Ordering::Relaxed);
    let mut tail = [0; 2];
    ring.read(caller, 14, &mut tail).await?;
    if head == u16::from_le_bytes(tail) {
        return Err(Errno::ENOBUFS);
    }
    let mut bytes = [0; IoUringBuf::SIZE];
    ring.read(caller, u32::from(head & ring.mask) * 16, &mut bytes)
        .await?;
    let buf = IoUringBuf::read_from(&bytes).ok_or(Errno::EFAULT)?;
    let len = if len == 0 { buf.len } else { len.min(buf.len) };
    if buf
        .addr
        .checked_add(u64::from(len))
        .is_none_or(|end| end > 1 << 32)
    {
        return Err(Errno::ENOBUFS);
    }
    Ok(Pbuf {
        addr: buf.addr as u32,
        len,
        bid: buf.bid,
        ring: Some(ring),
        full_len: buf.len,
    })
}

/// `io_kbuf_commit`/`io_kbuf_inc_commit`: incremental buffers keep their id until
/// fewer than min_left bytes remain. EAGAIN recycles without advancing the head.
pub(super) async fn complete(
    caller: &Caller<'_>,
    buffer: Pbuf,
    consumed: u32,
) -> Result<u32, Errno> {
    let mut flags = IoringCqeFlags::IORING_CQE_F_BUFFER.0 | (u32::from(buffer.bid) << 16);
    if let Some(ring) = buffer.ring {
        let head = ring.head.load(Ordering::Relaxed);
        let left = buffer.full_len.saturating_sub(consumed);
        if ring.inc && (consumed == 0 || left >= ring.min_left || buffer.full_len == 0) {
            if consumed > 0 {
                let at = u32::from(head & ring.mask) * 16;
                // Keep bid/resv (in particular ring tail in slot 0) unchanged.
                ring.write(
                    caller,
                    at,
                    &(u64::from(buffer.addr) + u64::from(consumed)).to_le_bytes(),
                )
                .await?;
                ring.write(caller, at + 8, &left.to_le_bytes()).await?;
            }
            flags |= IoringCqeFlags::IORING_CQE_F_BUF_MORE.0;
        } else {
            if ring.inc {
                ring.write(
                    caller,
                    u32::from(head & ring.mask) * 16 + 8,
                    &0u32.to_le_bytes(),
                )
                .await?;
            }
            ring.head.store(head.wrapping_add(1), Ordering::Relaxed);
        }
    }
    Ok(flags)
}

pub(super) fn recycle(ctx: &Ctx, bgid: u16, mut buffer: Pbuf) {
    if buffer.ring.is_some() {
        return;
    }
    buffer.len = buffer.full_len;
    if let Some(Group::Legacy(list)) = ctx.state.lock_or_recover().pbufs.get_mut(&bgid) {
        list.push_front(buffer);
    }
}

pub(super) fn provide(ctx: &Ctx, sqe: &IoUringSqe) -> Result<i32, Errno> {
    if sqe.op_flags != 0 || sqe.splice_fd_in != 0 {
        return Err(Errno::EINVAL);
    }
    let nr = sqe.fd as u32;
    if nr == 0 || nr > 65536 {
        return Err(Errno::E2BIG);
    }
    if sqe.len == 0 {
        return Err(Errno::EINVAL);
    }
    let end = sqe
        .addr
        .checked_add(u64::from(sqe.len) * u64::from(nr))
        .ok_or(Errno::EOVERFLOW)?;
    if end > 1 << 32 {
        return Err(Errno::EFAULT);
    }
    if sqe.off > u64::from(u16::MAX) {
        return Err(Errno::E2BIG);
    }
    if sqe.off + u64::from(nr) > 65536 {
        return Err(Errno::EINVAL);
    }
    let mut state = ctx.state.lock_or_recover();
    let Group::Legacy(list) = state.pbufs.entry(sqe.buf_index).or_default() else {
        return Err(Errno::EINVAL);
    };
    if list.len() == 65536 {
        return Err(Errno::EOVERFLOW);
    }
    for i in 0..nr.min(65536 - list.len() as u32) {
        let len = sqe.len.min(crate::MAX_RW_COUNT as u32);
        list.push_back(Pbuf {
            addr: (sqe.addr + u64::from(i) * u64::from(sqe.len)) as u32,
            len,
            bid: (sqe.off + u64::from(i)) as u16,
            ring: None,
            full_len: len,
        });
    }
    Ok(0)
}

pub(super) fn remove(ctx: &Ctx, sqe: &IoUringSqe) -> Result<i32, Errno> {
    if sqe.op_flags != 0
        || sqe.addr != 0
        || sqe.len != 0
        || sqe.off != 0
        || sqe.splice_fd_in != 0
        || sqe.fd <= 0
        || sqe.fd as u32 > 65536
    {
        return Err(Errno::EINVAL);
    }
    let mut state = ctx.state.lock_or_recover();
    let Group::Legacy(list) = state.pbufs.get_mut(&sqe.buf_index).ok_or(Errno::ENOENT)? else {
        return Err(Errno::EINVAL);
    };
    let count = (sqe.fd as usize).min(list.len());
    list.drain(..count);
    Ok(count as i32)
}
