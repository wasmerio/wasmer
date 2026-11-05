//! `splice`, `tee` and `vmsplice` (`fs/splice.c`). Pipes hold bytes rather than pages here, so
//! data is copied where Linux moves or lends pages; what each call takes, gives and returns is
//! Linux's. `SPLICE_F_MOVE` and `SPLICE_F_GIFT` are hints Linux may ignore, and are ignored;
//! `SPLICE_F_MORE` only hints that more data follows.

use std::{cell::Cell, sync::Arc};

use wasixv2_abi::{ConstIovecArray, GuestPtr, Iovec, LoffPtr, OpenFlags, SpliceFlags};
use wasixv2_syscalls::{
    Caller, Errno, Fd, LocalFuture, SysErr, SysResult, TaskId, read_value, write_value,
};
use wasixv2_vfs::FileType;
use wasixv2_wait::Interrupt;

use super::io::{CopyOut, Cursor, read_iovecs};
use crate::{
    Kernel, MAX_RW_COUNT,
    file::{Backing, OpenFile, check_file_size},
    pipe::{Consumer, Moved, Pipe, ToUser, pipe_info},
};

/// `SPLICE_F_ALL`.
const SPLICE_F_ALL: u32 = SpliceFlags::SPLICE_F_MOVE.0
    | SpliceFlags::SPLICE_F_NONBLOCK.0
    | SpliceFlags::SPLICE_F_MORE.0
    | SpliceFlags::SPLICE_F_GIFT.0;

/// Where a splice reads or writes a file: at `*off` from the caller, which is written back, or
/// at the file position, which moves.
struct Offset {
    position: bool,
    pos: u64,
}

impl Kernel {
    /// `splice`: from a pipe to a pipe, a file or a socket, or into a pipe from a file or a
    /// socket.
    #[allow(clippy::too_many_arguments)] // mirrors the syscall
    pub(crate) async fn sys_splice(
        &self,
        caller: &Caller<'_>,
        fd_in: Fd,
        off_in: LoffPtr,
        fd_out: Fd,
        off_out: LoffPtr,
        len: u32,
        flags: SpliceFlags,
    ) -> SysResult<i32> {
        if len == 0 {
            return Ok(0);
        }
        if flags.0 & !SPLICE_F_ALL != 0 {
            return Err(Errno::EINVAL.into());
        }
        let input = self.io_file(caller, fd_in)?;
        let output = self.io_file(caller, fd_out)?;
        // fs/pipe.c get_pipe_info(for_splice=true) excludes watch queues.
        if pipe_info(&input).is_some_and(|p| p.is_notification())
            || pipe_info(&output).is_some_and(|p| p.is_notification())
        {
            return Err(Errno::EINVAL.into());
        }
        // `__do_splice`: a pipe has no offset; then the offsets are read, the output's first.
        let ipipe = pipe_info(&input);
        let opipe = pipe_info(&output);
        if (ipipe.is_some() && !off_in.is_null()) || (opipe.is_some() && !off_out.is_null()) {
            return Err(Errno::ESPIPE.into());
        }
        let out_at = user_offset(caller, off_out).await?;
        let in_at = user_offset(caller, off_in).await?;
        let (n, input_offset, output_offset) = self
            .splice_files(caller, &input, &output, in_at, out_at, len, flags)
            .await?;
        if let Some(at) = output_offset {
            write_value(caller.memory(), off_out, &at).await?;
        }
        if let Some(at) = input_offset {
            write_value(caller.memory(), off_in, &at).await?;
        }
        Ok(n)
    }

    /// io_uring/splice.c:io_splice uses offsets by value, without guest pointers.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn splice_files(
        &self,
        caller: &Caller<'_>,
        input: &OpenFile,
        output: &OpenFile,
        mut in_at: Option<i64>,
        mut out_at: Option<i64>,
        len: u32,
        flags: SpliceFlags,
    ) -> SysResult<(i32, Option<i64>, Option<i64>)> {
        if len == 0 {
            return Ok((0, in_at, out_at));
        }
        if flags.0 & !SPLICE_F_ALL != 0 {
            return Err(Errno::EINVAL.into());
        }
        let (ipipe, opipe) = (pipe_info(input), pipe_info(output));
        // fs/pipe.c:get_pipe_info(for_splice=true), including io_uring/splice.c:io_splice.
        if ipipe.as_ref().is_some_and(|p| p.is_notification())
            || opipe.as_ref().is_some_and(|p| p.is_notification())
        {
            return Err(Errno::EINVAL.into());
        }
        if (ipipe.is_some() && in_at.is_some()) || (opipe.is_some() && out_at.is_some()) {
            return Err(Errno::ESPIPE.into());
        }
        // `do_splice`.
        input.check_readable()?;
        output.check_writable()?;
        let len = (len as usize).min(MAX_RW_COUNT);
        let mut nonblock = flags.contains(SpliceFlags::SPLICE_F_NONBLOCK);
        let nonblocking = |file: &OpenFile| file.flags().contains(OpenFlags::O_NONBLOCK);
        let spliced = match (ipipe, opipe) {
            (Some(ipipe), Some(opipe)) => {
                if Arc::ptr_eq(&ipipe, &opipe) {
                    return Err(Errno::EINVAL.into());
                }
                nonblock |= nonblocking(input) || nonblocking(output);
                self.splice_pipe_to_pipe(caller, &ipipe, &opipe, len, nonblock)
                    .await?
            }
            (Some(ipipe), None) => {
                if out_at.is_some() && output.check_seekable().is_err() {
                    return Err(Errno::EINVAL.into());
                }
                if output.flags().contains(OpenFlags::O_APPEND) {
                    return Err(Errno::EINVAL.into());
                }
                nonblock |= nonblocking(input);
                let _pos = self.lock_file_pos(output, out_at).await;
                let mut at = Offset::new(output, out_at, len)?;
                let spliced = self
                    .splice_from_pipe(caller, &ipipe, output, &mut at, len, nonblock)
                    .await?;
                if out_at.is_some() {
                    out_at = Some(at.pos as i64);
                }
                Self::put_offset(output, at);
                spliced
            }
            (None, Some(opipe)) => {
                if in_at.is_some() && input.check_seekable().is_err() {
                    return Err(Errno::EINVAL.into());
                }
                nonblock |= nonblocking(output);
                let _pos = self.lock_file_pos(input, in_at).await;
                let mut at = Offset::new(input, in_at, len)?;
                let spliced = self
                    .splice_to_pipe(caller, input, &opipe, &mut at, len, nonblock)
                    .await?;
                if in_at.is_some() {
                    in_at = Some(at.pos as i64);
                }
                Self::put_offset(input, at);
                spliced
            }
            (None, None) => return Err(Errno::EINVAL.into()),
        };
        Ok((spliced as i32, in_at, out_at))
    }

    /// `tee`: copies at most `len` bytes from the front of one pipe to the end of another and
    /// leaves them in the first.
    pub(crate) async fn sys_tee(
        &self,
        caller: &Caller<'_>,
        fd_in: Fd,
        fd_out: Fd,
        len: u32,
        flags: SpliceFlags,
    ) -> SysResult<i32> {
        if flags.0 & !SPLICE_F_ALL != 0 {
            return Err(Errno::EINVAL.into());
        }
        if len == 0 {
            return Ok(0);
        }
        let input = self.io_file(caller, fd_in)?;
        let output = self.io_file(caller, fd_out)?;
        self.tee_files(caller, &input, &output, len, flags).await
    }

    pub(crate) async fn tee_files(
        &self,
        caller: &Caller<'_>,
        input: &OpenFile,
        output: &OpenFile,
        len: u32,
        flags: SpliceFlags,
    ) -> SysResult<i32> {
        if flags.0 & !SPLICE_F_ALL != 0 {
            return Err(Errno::EINVAL.into());
        }
        if len == 0 {
            return Ok(0);
        }
        // fs/pipe.c get_pipe_info(for_splice=true) excludes watch queues.
        if pipe_info(input).is_some_and(|p| p.is_notification())
            || pipe_info(output).is_some_and(|p| p.is_notification())
        {
            return Err(Errno::EINVAL.into());
        }
        // `do_tee`.
        input.check_readable()?;
        output.check_writable()?;
        let (Some(ipipe), Some(opipe)) = (pipe_info(input), pipe_info(output)) else {
            return Err(Errno::EINVAL.into());
        };
        if Arc::ptr_eq(&ipipe, &opipe) {
            return Err(Errno::EINVAL.into());
        }
        let nonblock = flags.contains(SpliceFlags::SPLICE_F_NONBLOCK)
            || input.flags().contains(OpenFlags::O_NONBLOCK)
            || output.flags().contains(OpenFlags::O_NONBLOCK);
        let interrupt = self.interrupt(caller);
        ipipe.wait_readable(nonblock, &*interrupt).await?;
        self.wait_room(caller, &opipe, nonblock, &*interrupt)
            .await?;
        // `link_pipe` takes what is there now, possibly nothing.
        match ipipe.link_to(&opipe, len.min(MAX_RW_COUNT as u32) as usize) {
            Moved::Bytes(n) => Ok(n as i32),
            Moved::InEmpty { .. } | Moved::OutFull => Ok(0),
            Moved::NoReaders => {
                self.send_sigpipe(caller);
                Err(Errno::EPIPE.into())
            }
        }
    }

    /// `vmsplice`: the user's buffers into a pipe the descriptor writes, or out of a pipe it
    /// only reads. Pages are copied rather than lent, which Linux allows; only
    /// `SPLICE_F_NONBLOCK` makes it non-blocking, not the descriptor's `O_NONBLOCK`.
    pub(crate) async fn sys_vmsplice(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        uiov: ConstIovecArray,
        nr_segs: u32,
        flags: SpliceFlags,
    ) -> SysResult<i32> {
        if flags.0 & !SPLICE_F_ALL != 0 {
            return Err(Errno::EINVAL.into());
        }
        let file = self.io_file(caller, fd)?;
        let to_pipe = match (file.check_writable(), file.check_readable()) {
            (Ok(()), _) => true,
            (Err(_), Ok(())) => false,
            _ => return Err(Errno::EBADF.into()),
        };
        let iovecs = read_iovecs(caller.memory(), uiov, nr_segs).await?;
        let total: usize = iovecs.iter().map(|iov| iov.iov_len as usize).sum();
        if total == 0 {
            return Ok(0);
        }
        let pipe = pipe_info(&file)
            .filter(|p| !p.is_notification())
            .ok_or(Errno::EBADF)?;
        let nonblock = flags.contains(SpliceFlags::SPLICE_F_NONBLOCK);
        let n = match to_pipe {
            true => {
                self.vmsplice_to_pipe(caller, &pipe, &iovecs, total, nonblock)
                    .await?
            }
            false => {
                self.vmsplice_to_user(caller, &pipe, &iovecs, total, nonblock)
                    .await?
            }
        };
        Ok(n as i32)
    }

    /// `vmsplice_to_pipe`: waits for room once, then takes as much of the buffers as fits.
    async fn vmsplice_to_pipe(
        &self,
        caller: &Caller<'_>,
        pipe: &Arc<Pipe>,
        iovecs: &[Iovec],
        total: usize,
        nonblock: bool,
    ) -> SysResult<usize> {
        let interrupt = self.interrupt(caller);
        let mut cursor = Cursor::new(iovecs);
        loop {
            self.wait_room(caller, pipe, nonblock, &*interrupt).await?;
            let room = pipe.reserve(total);
            if room.len() == 0 {
                continue;
            }
            let mut buf = vec![0; room.len()];
            let (n, faulted) = cursor.copy_in(caller.memory(), &mut buf).await;
            room.fill(&buf[..n]);
            if n == 0 && faulted {
                return Err(Errno::EFAULT.into());
            }
            return Ok(n);
        }
    }

    /// `vmsplice_to_user`: a read of the pipe into the buffers.
    async fn vmsplice_to_user(
        &self,
        caller: &Caller<'_>,
        pipe: &Pipe,
        iovecs: &[Iovec],
        total: usize,
        nonblock: bool,
    ) -> SysResult<usize> {
        let interrupt = self.interrupt(caller);
        let mut to = CopyOut {
            cursor: Cursor::new(iovecs),
            memory: caller.memory(),
        };
        let mut buf = vec![0; total.min(pipe.capacity())];
        pipe.consume(nonblock, &*interrupt, &mut buf, &mut ToUser(&mut to), false)
            .await
    }

    /// `splice_pipe_to_pipe`: waits for data and for room, then moves what fits, and waits
    /// again if another task took either first.
    async fn splice_pipe_to_pipe(
        &self,
        caller: &Caller<'_>,
        ipipe: &Pipe,
        opipe: &Pipe,
        len: usize,
        nonblock: bool,
    ) -> SysResult<usize> {
        let interrupt = self.interrupt(caller);
        loop {
            ipipe.wait_readable(nonblock, &*interrupt).await?;
            self.wait_room(caller, opipe, nonblock, &*interrupt).await?;
            match ipipe.move_to(opipe, len).await {
                Moved::Bytes(n) => return Ok(n),
                Moved::InEmpty { writers: false } => return Ok(0),
                Moved::InEmpty { writers: true } | Moved::OutFull if nonblock => {
                    return Err(Errno::EAGAIN.into());
                }
                Moved::InEmpty { .. } | Moved::OutFull => {}
                Moved::NoReaders => {
                    self.send_sigpipe(caller);
                    return Err(Errno::EPIPE.into());
                }
            }
        }
    }

    /// `do_splice_from`: what the pipe holds, up to `len` bytes, to `output` at `at` (a file)
    /// or as a write (a socket or device); only what `output` took leaves the pipe.
    async fn splice_from_pipe(
        &self,
        caller: &Caller<'_>,
        ipipe: &Pipe,
        output: &OpenFile,
        at: &mut Offset,
        len: usize,
        nonblock: bool,
    ) -> SysResult<usize> {
        // FUSE takes a complete reply before validation; generic splice_write
        // instead leaves bytes queued on failure (fs/fuse/dev.c:fuse_dev_splice_write).
        if let Some(device) = output.downcast::<crate::fuse::Device>() {
            return device.splice_from_pipe(ipipe, len).await;
        }
        let writes = match &output.backing {
            Backing::Inode(file) => file.file_type() == FileType::Regular,
            Backing::Stream(object, _) => object.splice_write(),
        };
        if !writes {
            return Err(Errno::EINVAL.into());
        }
        let interrupt = self.interrupt(caller);
        let broken = Cell::new(false);
        self.pre_content_permission(caller, output, Some(at.pos), len as u64)
            .await?;
        self.sync_file_mappings(caller, output).await;
        let guard = match &output.backing {
            Backing::Inode(file) if file.limits_file_size() => Some(file.write_guard().await?),
            _ => None,
        };
        // fs/splice.c:iter_file_splice_write checks write limits before
        // consuming pipe buffers through generic_file_write_iter.
        let len = if guard.is_some() {
            self.file_write_count(caller, at.pos, len)?
        } else {
            len
        };
        let mut out = SpliceOut {
            guard: guard.as_ref(),
            cred: self.env(caller).cred,
            task: caller.task(),
            output,
            pos: &mut at.pos,
            interrupt: &*interrupt,
            broken: &broken,
        };
        let mut buf = vec![0; len.min(ipipe.capacity())];
        let spliced = ipipe
            .consume(nonblock, &*interrupt, &mut buf, &mut out, false)
            .await;
        drop(guard);
        self.sync_file_mappings(caller, output).await;
        self.epipe(caller, output, spliced, broken.get())
    }

    /// `splice_file_to_pipe`: waits for room in the pipe, then reads at most what fits from
    /// `input` at `at` (a file or device) or as a read (a socket), once.
    async fn splice_to_pipe(
        &self,
        caller: &Caller<'_>,
        input: &OpenFile,
        opipe: &Arc<Pipe>,
        at: &mut Offset,
        len: usize,
        nonblock: bool,
    ) -> SysResult<usize> {
        let interrupt = self.interrupt(caller);
        loop {
            self.wait_room(caller, opipe, nonblock, &*interrupt).await?;
            // `do_splice_read`: only what has a `splice_read`.
            let (inode, positional) = match &input.backing {
                Backing::Inode(file) if file.file_type() == FileType::Regular => (Some(file), true),
                Backing::Inode(_) => return Err(Errno::EINVAL.into()),
                Backing::Stream(object, _) if object.splice_read() => {
                    (None, input.check_seekable().is_ok())
                }
                Backing::Stream(..) => return Err(Errno::EINVAL.into()),
            };
            let room = opipe.reserve(len);
            if room.len() == 0 {
                continue;
            }
            let mut buf = vec![0; room.len()];
            self.pre_content_permission(caller, input, Some(at.pos), len as u64)
                .await?;
            self.access_permission(caller, input).await?;
            self.sync_file_mappings(caller, input).await;
            let n = match (inode, positional) {
                (Some(file), _) => file.read_at(&mut buf, at.pos).await?,
                (None, true) => input.read_at(caller.task(), &mut buf, at.pos).await?,
                (None, false) => {
                    let mut io = input.io(caller.task(), &*interrupt);
                    // fuse_dev_do_read checks only the device file's O_NONBLOCK;
                    // SPLICE_F_NONBLOCK governs pipe room, not waiting for a request.
                    if nonblock && input.downcast::<crate::fuse::Device>().is_none() {
                        io.flags |= OpenFlags::O_NONBLOCK;
                    }
                    let object = input.object().expect("a stream has an object");
                    object.read(io, &mut buf).await?
                }
            };
            room.fill(&buf[..n]);
            at.pos += n as u64;
            return Ok(n);
        }
    }

    /// `wait_for_space` on `pipe`, raising `SIGPIPE` when it has no reader.
    async fn wait_room(
        &self,
        caller: &Caller<'_>,
        pipe: &Pipe,
        nonblock: bool,
        interrupt: &dyn Interrupt,
    ) -> SysResult<()> {
        let room = pipe.wait_room(nonblock, interrupt).await;
        if room == Err(SysErr::Errno(Errno::EPIPE)) {
            self.send_sigpipe(caller);
        }
        room
    }

    /// The position lock of a file whose position the splice uses and moves (`fdget_pos`).
    async fn lock_file_pos<'a>(
        &self,
        file: &'a OpenFile,
        at: Option<i64>,
    ) -> Option<futures_util::lock::MutexGuard<'a, ()>> {
        match (at, &file.backing) {
            (None, Backing::Inode(_)) => Some(file.lock_pos().await),
            _ => None,
        }
    }

    /// Writes back where the splice ended: to the caller's offset, or as the position of a
    /// file that has one.
    fn put_offset(file: &OpenFile, at: Offset) {
        if at.position && file.check_seekable().is_ok() {
            file.set_pos(at.pos);
        }
    }
}

impl Offset {
    /// The offset a splice of `len` bytes starts at, from the caller or the file position,
    /// checked as `rw_verify_area` does: not negative, and not past the largest offset.
    fn new(file: &OpenFile, at: Option<i64>, len: usize) -> Result<Self, Errno> {
        let pos = match at {
            Some(at) => at,
            None => file.pos() as i64,
        };
        if pos < 0 || pos.checked_add(len as i64).is_none() {
            return Err(Errno::EINVAL);
        }
        Ok(Self {
            position: at.is_none(),
            pos: pos as u64,
        })
    }
}

/// `*ptr`, or `None` for a null `ptr`.
async fn user_offset(caller: &Caller<'_>, ptr: LoffPtr) -> Result<Option<i64>, Errno> {
    if ptr.is_null() {
        return Ok(None);
    }
    let at = read_value(caller.memory(), GuestPtr::new(ptr.addr()))
        .await
        .map_err(|_| Errno::EFAULT)?;
    Ok(Some(at))
}

/// A splice's output as a pipe consumer: a regular file written at `pos`, which moves, or a
/// socket or device written as `write` does (`iter_file_splice_write`, `splice_to_socket`).
struct SpliceOut<'a> {
    guard: Option<&'a wasixv2_vfs::FileWriteGuard<'a>>,
    cred: wasixv2_vfs::Cred,
    task: TaskId,
    output: &'a OpenFile,
    pos: &'a mut u64,
    interrupt: &'a dyn Interrupt,
    broken: &'a Cell<bool>,
}

impl Consumer for SpliceOut<'_> {
    fn take<'a>(&'a mut self, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move {
            let Backing::Inode(file) = &self.output.backing else {
                return self
                    .output
                    .write(self.task, self.interrupt, self.broken, data)
                    .await;
            };
            check_file_size(*self.pos, data.len())?;
            let n = match self.guard {
                Some(guard) => guard.write_at_as(data, *self.pos, &self.cred).await?,
                None => file.write_at_as(data, *self.pos, &self.cred).await?,
            };
            *self.pos += n as u64;
            Ok(n)
        })
    }
}
