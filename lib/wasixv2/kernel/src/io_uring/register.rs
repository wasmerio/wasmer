//! `io_uring_register` (`io_uring/register.c`, `rsrc.c`, `eventfd.c`): registered files and
//! buffers, the eventfd, the probe and enabling a disabled ring.

use std::sync::Arc;

use wasixv2_abi::{
    GuestLayout, GuestPtr, IoUringFilesUpdate, IoUringMisc, IoUringOp, IoUringProbe,
    IoUringProbeOp, IoUringRegisterOp, IoUringRsrcRegister, IoUringRsrcUpdate2, IoringSetupFlags,
    Iovec,
};
use wasixv2_syscalls::{Caller, Errno, Fd, SysResult, read_value};

use super::{Ctx, is_ring, ops::SUPPORTED, rsrc::Node};
use crate::{Kernel, cred::Cred, file::OpenFile, sync::MutexExt};

/// `IORING_MAX_FIXED_FILES`.
const MAX_FIXED_FILES: u32 = 1 << 20;
/// `IORING_MAX_REG_BUFFERS`.
const MAX_REG_BUFFERS: u32 = 1 << 14;
/// A registered buffer is at most 1 GiB (`io_buffer_validate`).
const MAX_BUFFER: u32 = 1 << 30;

impl Kernel {
    /// `__io_uring_register`.
    pub(super) async fn register(
        &self,
        caller: &Caller<'_>,
        ctx: &Arc<Ctx>,
        opcode: u32,
        arg: u32,
        nr_args: u32,
    ) -> SysResult<i32> {
        match IoUringRegisterOp(opcode) {
            IoUringRegisterOp::IORING_REGISTER_PBUF_RING
            | IoUringRegisterOp::IORING_UNREGISTER_PBUF_RING
            | IoUringRegisterOp::IORING_REGISTER_PBUF_STATUS => {
                if arg == 0 || nr_args != 1 {
                    return Err(Errno::EINVAL.into());
                }
                match IoUringRegisterOp(opcode) {
                    IoUringRegisterOp::IORING_REGISTER_PBUF_RING => {
                        self.register_pbuf_ring(caller, ctx, arg).await
                    }
                    IoUringRegisterOp::IORING_UNREGISTER_PBUF_RING => {
                        self.unregister_pbuf_ring(caller, ctx, arg).await
                    }
                    _ => self.pbuf_status(caller, ctx, arg).await,
                }
            }
            IoUringRegisterOp::IORING_REGISTER_BUFFERS => {
                if arg == 0 {
                    return Err(Errno::EFAULT.into());
                }
                self.register_buffers(caller, ctx, u64::from(arg), nr_args, 0)
                    .await
            }
            IoUringRegisterOp::IORING_REGISTER_FILES2
            | IoUringRegisterOp::IORING_REGISTER_BUFFERS2 => {
                if nr_args as usize != IoUringRsrcRegister::SIZE {
                    return Err(Errno::EINVAL.into());
                }
                let rr: IoUringRsrcRegister =
                    read_value(caller.memory(), GuestPtr::new(arg)).await?;
                if rr.nr == 0
                    || rr.resv2 != 0
                    || rr.flags & !1 != 0
                    || (rr.flags & 1 != 0 && rr.data != 0)
                {
                    return Err(Errno::EINVAL.into());
                }
                if opcode == IoUringRegisterOp::IORING_REGISTER_FILES2.0 {
                    self.register_files_tagged(caller, ctx, rr.data, rr.nr, rr.tags)
                        .await
                } else {
                    self.register_buffers(caller, ctx, rr.data, rr.nr, rr.tags)
                        .await
                }
            }
            IoUringRegisterOp::IORING_REGISTER_FILES_UPDATE2
            | IoUringRegisterOp::IORING_REGISTER_BUFFERS_UPDATE => {
                if nr_args as usize != IoUringRsrcUpdate2::SIZE {
                    return Err(Errno::EINVAL.into());
                }
                let up: IoUringRsrcUpdate2 =
                    read_value(caller.memory(), GuestPtr::new(arg)).await?;
                self.update_resources(
                    caller,
                    ctx,
                    up,
                    opcode == IoUringRegisterOp::IORING_REGISTER_BUFFERS_UPDATE.0,
                )
                .await
            }
            IoUringRegisterOp::IORING_REGISTER_FILE_ALLOC_RANGE => {
                if arg == 0 || nr_args != 0 {
                    return Err(Errno::EINVAL.into());
                }
                self.register_alloc_range(caller, ctx, arg).await
            }
            IoUringRegisterOp::IORING_UNREGISTER_BUFFERS => {
                if arg != 0 || nr_args != 0 {
                    return Err(Errno::EINVAL.into());
                }
                let buffers = ctx.state.lock_or_recover().buffers.take();
                drop(buffers.ok_or(Errno::ENXIO)?);
                Ok(0)
            }
            IoUringRegisterOp::IORING_REGISTER_FILES => {
                self.register_files(caller, ctx, arg, nr_args).await
            }
            IoUringRegisterOp::IORING_UNREGISTER_FILES => {
                if arg != 0 || nr_args != 0 {
                    return Err(Errno::EINVAL.into());
                }
                let files = ctx.state.lock_or_recover().files.take();
                files.ok_or(Errno::ENXIO)?;
                Ok(0)
            }
            IoUringRegisterOp::IORING_REGISTER_FILES_UPDATE => {
                self.update_files(caller, ctx, arg, nr_args).await
            }
            IoUringRegisterOp::IORING_REGISTER_EVENTFD
            | IoUringRegisterOp::IORING_REGISTER_EVENTFD_ASYNC => {
                if nr_args != 1 {
                    return Err(Errno::EINVAL.into());
                }
                let fd: i32 = read_value(caller.memory(), GuestPtr::new(arg)).await?;
                if ctx.state.lock_or_recover().eventfd.is_some() {
                    return Err(Errno::EBUSY.into());
                }
                let eventfd = self.eventfd_ctx_fdget(caller, Fd(fd))?;
                let only_async = opcode == IoUringRegisterOp::IORING_REGISTER_EVENTFD_ASYNC.0;
                let mut state = ctx.state.lock_or_recover();
                if state.eventfd.is_some() {
                    return Err(Errno::EBUSY.into());
                }
                state.eventfd = Some((eventfd, only_async));
                Ok(0)
            }
            IoUringRegisterOp::IORING_UNREGISTER_EVENTFD => {
                if arg != 0 || nr_args != 0 {
                    return Err(Errno::EINVAL.into());
                }
                let eventfd = ctx.state.lock_or_recover().eventfd.take();
                eventfd.ok_or(Errno::ENXIO)?;
                Ok(0)
            }
            IoUringRegisterOp::IORING_REGISTER_PROBE => {
                if arg == 0 || nr_args > 256 {
                    return Err(Errno::EINVAL.into());
                }
                self.probe(caller, arg, nr_args).await
            }
            IoUringRegisterOp::IORING_REGISTER_PERSONALITY => {
                if arg != 0 || nr_args != 0 {
                    return Err(Errno::EINVAL.into());
                }
                let cred = self.cred(caller);
                Ok(ctx.register_personality((*cred).clone())?)
            }
            IoUringRegisterOp::IORING_UNREGISTER_PERSONALITY => {
                if arg != 0 {
                    return Err(Errno::EINVAL.into());
                }
                let id = u16::try_from(nr_args).map_err(|_| Errno::EINVAL)?;
                let creds = ctx.state.lock_or_recover().personalities.remove(&id);
                creds.ok_or(Errno::EINVAL)?;
                Ok(0)
            }
            IoUringRegisterOp::IORING_REGISTER_ENABLE_RINGS => {
                if arg != 0 || nr_args != 0 {
                    return Err(Errno::EINVAL.into());
                }
                // `io_register_enable_rings`: the single issuer is whoever enables the ring.
                let single = ctx.shape.has(IoringSetupFlags::IORING_SETUP_SINGLE_ISSUER);
                let mut state = ctx.state.lock_or_recover();
                if !state.disabled {
                    return Err(Errno::EBADFD.into());
                }
                if single {
                    state.submitter = Some(caller.task());
                }
                state.disabled = false;
                Ok(0)
            }
            IoUringRegisterOp::IORING_REGISTER_RING_FDS => {
                self.ringfd_register(caller, ctx, arg, nr_args).await
            }
            IoUringRegisterOp::IORING_UNREGISTER_RING_FDS => {
                self.ringfd_unregister(caller, arg, nr_args).await
            }
            _ => Err(Errno::EINVAL.into()),
        }
    }

    /// `io_sqe_files_register`: `nr_args` descriptors from `arg`; -1 leaves a slot empty.
    /// More than `RLIMIT_NOFILE` is `EMFILE`; a ring is `EBADF`.
    async fn register_files(
        &self,
        caller: &Caller<'_>,
        ctx: &Ctx,
        arg: u32,
        nr_args: u32,
    ) -> SysResult<i32> {
        self.register_files_tagged(caller, ctx, u64::from(arg), nr_args, 0)
            .await
    }

    async fn register_files_tagged(
        &self,
        caller: &Caller<'_>,
        ctx: &Ctx,
        arg: u64,
        nr_args: u32,
        tags: u64,
    ) -> SysResult<i32> {
        if ctx.state.lock_or_recover().files.is_some() {
            return Err(Errno::EBUSY.into());
        }
        if nr_args == 0 {
            return Err(Errno::EINVAL.into());
        }
        let nofile = self.files(caller.task()).lock_or_recover().limit();
        if nr_args > MAX_FIXED_FILES || nr_args as usize > nofile {
            return Err(Errno::EMFILE.into());
        }
        let mut files = Vec::with_capacity(nr_args as usize);
        for i in 0..nr_args {
            let tag = read_tag(caller, tags, i).await?;
            let fd = match arg {
                0 => -1,
                arg => {
                    read_value(caller.memory(), GuestPtr::<i32>::new(array_at(arg, i, 4)?)).await?
                }
            };
            let file = self.fixed_file(caller, fd)?;
            if file.is_none() && tag != 0 {
                return Err(Errno::EINVAL.into());
            }
            files.push((file, tag));
        }
        let mut state = ctx.state.lock_or_recover();
        if state.files.is_some() {
            return Err(Errno::EBUSY.into());
        }
        state.files = Some(
            files
                .into_iter()
                .map(|(file, tag)| file.map(|f| Node::new(ctx, f, tag)))
                .collect(),
        );
        state.file_alloc_start = 0;
        state.file_alloc_end = nr_args;
        state.file_alloc_hint = 0;
        Ok(0)
    }

    /// One slot of a registered file table: -1 (`IORING_REGISTER_FILES_SKIP` aside) is
    /// empty; a ring cannot be registered.
    pub(super) fn fixed_file(
        &self,
        caller: &Caller<'_>,
        fd: i32,
    ) -> Result<Option<Arc<OpenFile>>, Errno> {
        if fd == -1 {
            return Ok(None);
        }
        let file = self.file(caller.task(), Fd(fd))?;
        if is_ring(&file) {
            return Err(Errno::EBADF);
        }
        Ok(Some(file))
    }

    /// `io_register_files_update`: `nr_args` descriptors for the slots from `offset`; returns
    /// how many were updated. `IORING_REGISTER_FILES_SKIP` leaves a slot as it is.
    async fn update_files(
        &self,
        caller: &Caller<'_>,
        ctx: &Ctx,
        arg: u32,
        nr_args: u32,
    ) -> SysResult<i32> {
        if nr_args == 0 {
            return Err(Errno::EINVAL.into());
        }
        let update: IoUringFilesUpdate = read_value(caller.memory(), GuestPtr::new(arg)).await?;
        let up = IoUringRsrcUpdate2 {
            offset: update.offset,
            resv: update.resv,
            data: update.fds,
            nr: nr_args,
            ..IoUringRsrcUpdate2::ZERO
        };
        self.update_resources(caller, ctx, up, false).await
    }

    async fn register_buffers(
        &self,
        caller: &Caller<'_>,
        ctx: &Ctx,
        arg: u64,
        nr: u32,
        tags: u64,
    ) -> SysResult<i32> {
        if ctx.state.lock_or_recover().buffers.is_some() {
            return Err(Errno::EBUSY.into());
        }
        if nr == 0 || nr > MAX_REG_BUFFERS {
            return Err(Errno::EINVAL.into());
        }
        let mut buffers = Vec::with_capacity(nr as usize);
        for i in 0..nr {
            let iov = if arg == 0 {
                Iovec::ZERO
            } else {
                read_value(
                    caller.memory(),
                    GuestPtr::new(array_at(arg, i, Iovec::SIZE as u32)?),
                )
                .await?
            };
            let tag = read_tag(caller, tags, i).await?;
            let buffer = validate_buffer(iov)?;
            if buffer.is_none() && tag != 0 {
                return Err(Errno::EINVAL.into());
            }
            let buffer = match buffer {
                Some(iov) => Some(self.retain_pages(caller, iov.iov_base, iov.iov_len).await?),
                None => None,
            };
            buffers.push((buffer, tag));
        }
        let mut state = ctx.state.lock_or_recover();
        if state.buffers.is_some() {
            return Err(Errno::EBUSY.into());
        }
        state.buffers = Some(
            buffers
                .into_iter()
                .map(|(buffer, tag)| buffer.map(|b| Node::new(ctx, b, tag)))
                .collect(),
        );
        Ok(0)
    }

    /// `io_probe`: the result buffer must come zeroed; each of up to `IORING_OP_LAST`
    /// operations is reported, supported or not.
    async fn probe(&self, caller: &Caller<'_>, arg: u32, nr_args: u32) -> SysResult<i32> {
        let last = u32::from(IoUringOp::IORING_OP_LAST.0);
        let nr = nr_args.min(last);
        let size = IoUringProbe::SIZE + nr as usize * IoUringProbeOp::SIZE;
        let mut bytes = vec![0; size];
        caller.memory().read(arg, &mut bytes).await?;
        if bytes.iter().any(|&b| b != 0) {
            return Err(Errno::EINVAL.into());
        }
        let header = IoUringProbe {
            last_op: (last - 1) as u8,
            ops_len: nr as u8,
            resv: 0,
            resv2: [0; 3],
        };
        header.write_to(&mut bytes);
        for i in 0..nr {
            let supported = SUPPORTED.iter().any(|o| u32::from(o.0) == i);
            let entry = IoUringProbeOp {
                op: i as u8,
                resv: 0,
                flags: if supported {
                    IoUringMisc::IO_URING_OP_SUPPORTED.0 as u16
                } else {
                    0
                },
                resv2: 0,
            };
            let at = IoUringProbe::SIZE + i as usize * IoUringProbeOp::SIZE;
            entry.write_to(&mut bytes[at..]);
        }
        caller.memory().write(arg, &bytes).await?;
        Ok(0)
    }
}

fn array_at(base: u64, i: u32, size: u32) -> Result<u32, Errno> {
    u32::try_from(
        base.checked_add(u64::from(i) * u64::from(size))
            .ok_or(Errno::EFAULT)?,
    )
    .map_err(|_| Errno::EFAULT)
}

async fn read_tag(caller: &Caller<'_>, tags: u64, i: u32) -> SysResult<u64> {
    if tags == 0 {
        Ok(0)
    } else {
        Ok(read_value(caller.memory(), GuestPtr::new(array_at(tags, i, 8)?)).await?)
    }
}

/// `io_sqe_buffer_register`: (0,0) is a sparse slot; other buffers must be nonempty.
pub(super) fn validate_buffer(iov: Iovec) -> Result<Option<Iovec>, Errno> {
    if iov.iov_base == 0 && iov.iov_len == 0 {
        return Ok(None);
    }
    if iov.iov_base == 0
        || iov.iov_len == 0
        || iov.iov_len > MAX_BUFFER
        || u64::from(iov.iov_base) + u64::from(iov.iov_len) > 1 << 32
    {
        return Err(Errno::EFAULT);
    }
    Ok(Some(iov))
}

impl Ctx {
    /// `io_register_personality`: the caller's credentials under the next free id, cyclically
    /// from 1 up to `USHRT_MAX` (`xa_alloc_cyclic`); `EBUSY` once every id is taken.
    fn register_personality(&self, cred: Cred) -> Result<i32, Errno> {
        let mut state = self.state.lock_or_recover();
        let start = state.pers_next.max(1);
        let free = (start..=u16::MAX)
            .chain(1..start)
            .find(|id| !state.personalities.contains_key(id));
        let id = free.ok_or(Errno::EBUSY)?;
        state.personalities.insert(id, cred);
        state.pers_next = id.wrapping_add(1);
        Ok(i32::from(id))
    }
}
