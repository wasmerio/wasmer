//! `io_uring_register` (`io_uring/register.c`, `rsrc.c`, `eventfd.c`): registered files and
//! buffers, the eventfd, the probe and enabling a disabled ring.

use std::sync::Arc;

use wasixv2_abi::{
    GuestLayout, GuestPtr, IoUringFilesUpdate, IoUringMisc, IoUringOp, IoUringProbe,
    IoUringProbeOp, IoUringRegisterOp, Iovec,
};
use wasixv2_syscalls::{Caller, Errno, Fd, SysResult, read_value};

use super::{Ctx, is_ring, ops::SUPPORTED};
use crate::{Kernel, file::OpenFile, sync::MutexExt};

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
            IoUringRegisterOp::IORING_REGISTER_BUFFERS => {
                if arg == 0 || nr_args == 0 || nr_args > MAX_REG_BUFFERS {
                    return Err(Errno::EINVAL.into());
                }
                if ctx.state.lock_or_recover().buffers.is_some() {
                    return Err(Errno::EBUSY.into());
                }
                let iovecs = read_buffers(caller, arg, nr_args).await?;
                let mut state = ctx.state.lock_or_recover();
                if state.buffers.is_some() {
                    return Err(Errno::EBUSY.into());
                }
                state.buffers = Some(iovecs);
                Ok(0)
            }
            IoUringRegisterOp::IORING_UNREGISTER_BUFFERS => {
                if arg != 0 || nr_args != 0 {
                    return Err(Errno::EINVAL.into());
                }
                let mut state = ctx.state.lock_or_recover();
                state.buffers.take().ok_or(Errno::ENXIO)?;
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
            IoUringRegisterOp::IORING_REGISTER_PROBE => self.probe(caller, arg, nr_args).await,
            IoUringRegisterOp::IORING_REGISTER_ENABLE_RINGS => {
                let mut state = ctx.state.lock_or_recover();
                if !state.disabled {
                    return Err(Errno::EBADFD.into());
                }
                state.disabled = false;
                Ok(0)
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
        if ctx.state.lock_or_recover().files.is_some() {
            return Err(Errno::EBUSY.into());
        }
        if nr_args == 0 || nr_args > MAX_FIXED_FILES {
            return Err(Errno::EINVAL.into());
        }
        let nofile = self.files(caller.task()).lock_or_recover().limit();
        if nr_args as usize > nofile {
            return Err(Errno::EMFILE.into());
        }
        let mut files = Vec::with_capacity(nr_args as usize);
        for i in 0..nr_args {
            let fd = match arg {
                0 => -1,
                arg => read_value(caller.memory(), GuestPtr::<i32>::new(arg + i * 4)).await?,
            };
            files.push(self.fixed_file(caller, fd)?);
        }
        let mut state = ctx.state.lock_or_recover();
        if state.files.is_some() {
            return Err(Errno::EBUSY.into());
        }
        state.files = Some(files);
        Ok(0)
    }

    /// One slot of a registered file table: -1 (`IORING_REGISTER_FILES_SKIP` aside) is
    /// empty; a ring cannot be registered.
    fn fixed_file(&self, caller: &Caller<'_>, fd: i32) -> Result<Option<Arc<OpenFile>>, Errno> {
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
        if update.resv != 0 {
            return Err(Errno::EINVAL.into());
        }
        let len = match &ctx.state.lock_or_recover().files {
            Some(files) => files.len() as u32,
            None => return Err(Errno::ENXIO.into()),
        };
        if update
            .offset
            .checked_add(nr_args)
            .is_none_or(|end| end > len)
        {
            return Err(Errno::EINVAL.into());
        }
        let fds = u32::try_from(update.fds).map_err(|_| Errno::EFAULT)?;
        let skip = IoUringMisc::IORING_REGISTER_FILES_SKIP.0 as i32;
        let mut done = 0;
        for i in 0..nr_args {
            let Ok(fd) = read_value(caller.memory(), GuestPtr::<i32>::new(fds + i * 4)).await
            else {
                return match done {
                    0 => Err(Errno::EFAULT.into()),
                    done => Ok(done),
                };
            };
            if fd != skip {
                let file = match self.fixed_file(caller, fd) {
                    Ok(file) => file,
                    Err(errno) if done == 0 => return Err(errno.into()),
                    Err(_) => return Ok(done),
                };
                let old = {
                    let mut state = ctx.state.lock_or_recover();
                    let files = state.files.as_mut().ok_or(Errno::ENXIO)?;
                    std::mem::replace(&mut files[(update.offset + i) as usize], file)
                };
                drop(old);
            }
            done += 1;
        }
        Ok(done)
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

/// `io_sqe_buffers_register`'s iovecs: each at most 1 GiB, none empty unless its base is too.
async fn read_buffers(caller: &Caller<'_>, arg: u32, nr: u32) -> SysResult<Vec<(u32, u32)>> {
    let mut buffers = Vec::with_capacity(nr as usize);
    for i in 0..nr {
        let at = arg + i * Iovec::SIZE as u32;
        let iov: Iovec = read_value(caller.memory(), GuestPtr::new(at)).await?;
        if iov.iov_len > MAX_BUFFER {
            return Err(Errno::EFAULT.into());
        }
        if iov.iov_len == 0 && iov.iov_base != 0 {
            return Err(Errno::EFAULT.into());
        }
        if u64::from(iov.iov_base) + u64::from(iov.iov_len) > 1 << 32 {
            return Err(Errno::EFAULT.into());
        }
        buffers.push((iov.iov_base, iov.iov_len));
    }
    Ok(buffers)
}
