//! Data syscalls on open files (`fs/read_write.c`, `fs/readdir.c`, `fs/sync.c`, `fs/open.c`).

use wasixv2_abi::{
    ConstIovecArray, FadviseAdvice, FallocFlags, GuestLayout, GuestPtr, Iovec, LinuxDirent64,
    LinuxDirent64Ptr, LoffPtr, OpenFlags, RwfFlags, SeekWhence, SyncFileRangeFlags,
};
use wasixv2_syscalls::{
    Caller, Errno, Fd, LocalFuture, SysResult, TaskId, UserMemory, UserSlice, UserSliceMut,
    read_value, write_value,
};
use wasixv2_vfs::{AllocMode, DirEntry, DirSink, FileType};

use std::cell::Cell;

use wasixv2_wait::Interrupt;

use crate::{
    IO_CHUNK, Kernel, MAX_RW_COUNT,
    file::{Backing, OpenFile, ReadSink, SeekBase, check_file_size, delivered},
};

/// Most iovecs one `readv` or `writev` takes, as Linux's `UIO_MAXIOV`.
const IOV_MAX: u32 = 1024;

/// `RWF_SUPPORTED` minus the flags we cannot honour (`RWF_ATOMIC`, `RWF_DONTCACHE`).
const RWF_KNOWN: u32 = RwfFlags::RWF_HIPRI.0
    | RwfFlags::RWF_DSYNC.0
    | RwfFlags::RWF_SYNC.0
    | RwfFlags::RWF_NOWAIT.0
    | RwfFlags::RWF_APPEND.0
    | RwfFlags::RWF_NOAPPEND.0;

/// Where a vectored transfer happens.
#[derive(Clone, Copy)]
pub(crate) enum At {
    /// The file position, which moves.
    Position,
    /// An explicit offset; the position stays.
    Offset(u64),
}

/// Most bytes one datagram write may carry; a larger one is `EMSGSIZE`, as it would exceed
/// any socket's send buffer. Bounds the host allocation a guest-chosen length causes.
const MAX_MESSAGE: usize = 4 << 20;

thread_local! {
    /// This thread's buffer for the steps of reads and writes, kept between calls so that a
    /// call neither allocates nor zeroes one. A call that finds it taken (by a call suspended
    /// on this thread meanwhile) makes its own.
    static SCRATCH: Cell<Vec<u8>> = const { Cell::new(Vec::new()) };
}

/// A host buffer of `len` bytes for the steps of one call, from this thread's scratch. Only
/// bytes no call used before are zeroed; a step reads or writes just the part a transfer
/// filled. Buffers larger than one step (a datagram's) are neither taken nor kept.
struct Scratch {
    buf: Vec<u8>,
    len: usize,
}

impl Scratch {
    fn new(len: usize) -> Self {
        let mut buf = match len <= IO_CHUNK {
            true => SCRATCH.take(),
            false => Vec::new(),
        };
        if buf.len() < len {
            buf.resize(len, 0);
        }
        Self { buf, len }
    }
}

impl std::ops::Deref for Scratch {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

impl std::ops::DerefMut for Scratch {
    fn deref_mut(&mut self) -> &mut [u8] {
        &mut self.buf[..self.len]
    }
}

impl Drop for Scratch {
    /// Gives the buffer back, unless the scratch holds a larger one again.
    fn drop(&mut self) {
        if self.buf.len() > IO_CHUNK {
            return;
        }
        let kept = SCRATCH.take();
        SCRATCH.set(match kept.len() >= self.buf.len() {
            true => kept,
            false => std::mem::take(&mut self.buf),
        });
    }
}

/// A position in a list of guest buffers, for transfers made in steps.
pub(super) struct Cursor<'a> {
    iovecs: &'a [Iovec],
    index: usize,
    offset: usize,
}

impl<'a> Cursor<'a> {
    pub(super) fn new(iovecs: &'a [Iovec]) -> Self {
        Self {
            iovecs,
            index: 0,
            offset: 0,
        }
    }

    /// The guest buffer pieces covering the next `len` bytes, and advances past them.
    fn take(&mut self, mut len: usize) -> Vec<(u32, usize)> {
        let mut pieces = Vec::new();
        while len > 0 && self.index < self.iovecs.len() {
            let iov = &self.iovecs[self.index];
            let n = (iov.iov_len as usize - self.offset).min(len);
            if n > 0 {
                pieces.push((iov.iov_base.wrapping_add(self.offset as u32), n));
            }
            self.offset += n;
            len -= n;
            if self.offset == iov.iov_len as usize {
                self.index += 1;
                self.offset = 0;
            }
        }
        pieces
    }

    /// Copies the next bytes in from the guest, up to `buf.len()`. A fault ends the copy:
    /// the count before it and `true`.
    pub(super) async fn copy_in(
        &mut self,
        memory: &dyn UserMemory,
        buf: &mut [u8],
    ) -> (usize, bool) {
        let mut n = 0;
        for (addr, len) in self.take(buf.len()) {
            if memory.read(addr, &mut buf[n..n + len]).await.is_err() {
                return (n, true);
            }
            n += len;
        }
        (n, false)
    }

    /// Copies `data` out to the next guest bytes. A fault ends the copy: the count before it
    /// and `true`.
    async fn copy_out(&mut self, memory: &dyn UserMemory, data: &[u8]) -> (usize, bool) {
        let mut n = 0;
        for (addr, len) in self.take(data.len()) {
            if memory.write(addr, &data[n..n + len]).await.is_err() {
                return (n, true);
            }
            n += len;
        }
        (n, false)
    }
}

/// Guest buffers as a [`ReadSink`].
pub(super) struct CopyOut<'a> {
    pub(super) cursor: Cursor<'a>,
    pub(super) memory: &'a dyn UserMemory,
}

impl ReadSink for CopyOut<'_> {
    fn copy_out<'a>(&'a mut self, data: &'a [u8]) -> LocalFuture<'a, usize> {
        Box::pin(async move { self.cursor.copy_out(self.memory, data).await.0 })
    }
}

/// Where the steps of one read or write go. A transfer at the file position holds the
/// position lock throughout (`fdget_pos`), so threads sharing the description never interleave
/// within one call.
struct Target<'a> {
    task: TaskId,
    file: &'a OpenFile,
    interrupt: &'a dyn Interrupt,
    at: At,
    /// Writes go to the end of the file (`O_APPEND` or `RWF_APPEND`, unless `RWF_NOAPPEND`).
    append: bool,
    /// The offset the transfer started at, for files.
    start: u64,
    /// A stream write found the reading side gone ([`crate::file::Io::broken`]).
    broken: &'a Cell<bool>,
}

impl Target<'_> {
    /// Reads at most `buf.len()` bytes through `buf` to `to`; how many arrived. A file's
    /// position moves by what arrived, and a stream gives up only what arrived.
    async fn read_to(
        &self,
        buf: &mut [u8],
        done: usize,
        to: &mut dyn ReadSink,
    ) -> SysResult<usize> {
        let inner = match &self.file.backing {
            Backing::Inode(inner) => inner,
            Backing::Stream(object, _) => {
                return object
                    .read_to(self.file.io(self.task, self.interrupt), buf, to)
                    .await;
            }
        };
        let offset = self.start + done as u64;
        let n = inner.read_at(buf, offset).await?;
        let copied = to.copy_out(&buf[..n]).await;
        if matches!(self.at, At::Position) {
            self.file.set_pos(offset + copied as u64);
        }
        delivered(n, copied)
    }

    async fn write(&self, data: &[u8], done: usize) -> SysResult<usize> {
        let Backing::Inode(inner) = &self.file.backing else {
            return self
                .file
                .write(self.task, self.interrupt, self.broken, data)
                .await;
        };
        if self.append {
            let (end, n) = inner.append(data).await?;
            if matches!(self.at, At::Position) {
                self.file.set_pos(end);
            }
            return Ok(n);
        }
        let offset = self.start + done as u64;
        check_file_size(offset, data.len())?;
        let n = inner.write_at(data, offset).await?;
        if matches!(self.at, At::Position) {
            self.file.set_pos(offset + n as u64);
        }
        Ok(n)
    }
}

impl Kernel {
    /// Reads into the buffers in order. Files are read in steps until the buffers are full or
    /// the file ends, and so are objects that fill reads, which stop early at a signal with what
    /// they read so far; any other stream read returns what one read of the object gives, as
    /// Linux's `pipe_read` and socket reads do, into a buffer that holds what the object has
    /// (`FileObject::buffered`). A blocked read ends on a signal.
    pub(crate) async fn read_into_iovecs(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        iovecs: &[Iovec],
        at: At,
    ) -> SysResult<u32> {
        let total: usize = iovecs.iter().map(|iov| iov.iov_len as usize).sum();
        let interrupt = self.interrupt(caller);
        let _pos = match (at, &file.backing) {
            (At::Position, Backing::Inode(_)) => Some(file.lock_pos().await),
            _ => None,
        };
        let target = Target {
            task: caller.task(),
            file,
            interrupt: &*interrupt,
            at,
            append: false,
            start: match at {
                At::Position => file.pos(),
                At::Offset(offset) => offset,
            },
            broken: &Cell::new(false),
        };
        let steps = file.fills_reads();
        let step = match steps {
            true => IO_CHUNK,
            false => IO_CHUNK.max(file.buffered()),
        };
        let mut buf = Scratch::new(total.min(step));
        let mut to = CopyOut {
            cursor: Cursor::new(iovecs),
            memory: caller.memory(),
        };
        let mut done = 0;
        self.sync_file_mappings(caller, file).await;
        loop {
            let want = (total - done).min(buf.len());
            let copied = match target.read_to(&mut buf[..want], done, &mut to).await {
                Ok(copied) => copied,
                Err(_) if done > 0 => break,
                Err(err) => return Err(err),
            };
            done += copied;
            // Short: the end of the data, or a fault.
            if !steps || copied < want || done == total {
                break;
            }
            // `read_zero` and `get_random_bytes_user` look for a signal after each page.
            if file.object().is_some() && interrupt.pending() {
                break;
            }
        }
        Ok(done as u32)
    }

    /// Writes the buffers in order, in steps, until all is written or an object takes less:
    /// a blocking write to a stream writes everything, as Linux's do, while a signal, a full
    /// non-blocking object or a fault ends it with the count so far. A datagram is one write.
    /// A byte stream with no reader left raises `SIGPIPE`.
    async fn write_from_iovecs(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        iovecs: &[Iovec],
        at: At,
        flags: RwfFlags,
    ) -> SysResult<u32> {
        let broken = Cell::new(false);
        let ret = self
            .write_steps(caller, file, iovecs, at, flags, &broken)
            .await;
        self.epipe(caller, file, ret, broken.get())
    }

    async fn write_steps(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        iovecs: &[Iovec],
        at: At,
        flags: RwfFlags,
        broken: &Cell<bool>,
    ) -> SysResult<u32> {
        let total: usize = iovecs.iter().map(|iov| iov.iov_len as usize).sum();
        let messages = file.messages();
        if messages && total > MAX_MESSAGE {
            return Err(Errno::EMSGSIZE.into());
        }
        let interrupt = self.interrupt(caller);
        let _pos = match (at, &file.backing) {
            (At::Position, Backing::Inode(_)) => Some(file.lock_pos().await),
            _ => None,
        };
        let o_append = file.flags().contains(OpenFlags::O_APPEND);
        let target = Target {
            task: caller.task(),
            file,
            interrupt: &*interrupt,
            at,
            append: flags.contains(RwfFlags::RWF_APPEND)
                || (o_append && !flags.contains(RwfFlags::RWF_NOAPPEND)),
            start: match at {
                At::Position => file.pos(),
                At::Offset(offset) => offset,
            },
            broken,
        };
        let step = if messages { total } else { total.min(IO_CHUNK) };
        let mut buf = Scratch::new(step);
        let mut cursor = Cursor::new(iovecs);
        let mut done = 0;
        self.sync_file_mappings(caller, file).await;
        loop {
            let (n, faulted) = cursor.copy_in(caller.memory(), &mut buf).await;
            if n == 0 && faulted {
                if done == 0 {
                    return Err(Errno::EFAULT.into());
                }
                break;
            }
            let written = match target.write(&buf[..n], done).await {
                Ok(written) => written,
                Err(_) if done > 0 => break,
                Err(err) => return Err(err),
            };
            done += written;
            if messages || faulted || written < n || done == total {
                break;
            }
        }
        self.sync_file_mappings(caller, file).await;
        if flags.intersects(RwfFlags::RWF_DSYNC | RwfFlags::RWF_SYNC)
            && let Backing::Inode(inner) = &file.backing
        {
            inner.sync(!flags.contains(RwfFlags::RWF_SYNC)).await?;
        }
        Ok(done as u32)
    }

    /// `write(2)` on `file`, from one guest buffer.
    pub(crate) async fn write_buf(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        buf: UserSlice,
        at: At,
    ) -> SysResult<u32> {
        let iovec = [Iovec {
            iov_base: buf.addr,
            iov_len: buf.len.min(MAX_RW_COUNT as u32),
        }];
        self.write_from_iovecs(caller, file, &iovec, at, RwfFlags(0))
            .await
    }

    /// `read(2)` on `file`, into one guest buffer.
    pub(crate) async fn read_buf(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        buf: UserSliceMut,
        at: At,
    ) -> SysResult<u32> {
        let iovec = [Iovec {
            iov_base: buf.addr,
            iov_len: buf.len.min(MAX_RW_COUNT as u32),
        }];
        self.read_into_iovecs(caller, file, &iovec, at).await
    }

    /// `SIGPIPE` for an `EPIPE` from a byte stream (datagram and seqpacket sockets only
    /// fail), or for a pipe write that found no reader (`broken`) whatever it returns.
    pub(super) fn epipe<T>(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        ret: SysResult<T>,
        broken: bool,
    ) -> SysResult<T> {
        if broken {
            self.send_sigpipe(caller);
            return ret;
        }
        match file.messages() {
            true => ret,
            false => self.sigpipe(caller, ret),
        }
    }

    pub(crate) async fn sys_readv(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        vec: ConstIovecArray,
        vlen: u32,
    ) -> SysResult<u32> {
        let file = self.file(caller.task(), fd)?;
        file.check_readable()?;
        let iovecs = read_iovecs(caller.memory(), vec, vlen).await?;
        self.read_into_iovecs(caller, &file, &iovecs, At::Position)
            .await
    }

    pub(crate) async fn sys_writev(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        vec: ConstIovecArray,
        vlen: u32,
    ) -> SysResult<u32> {
        let file = self.file(caller.task(), fd)?;
        file.check_writable()?;
        let iovecs = read_iovecs(caller.memory(), vec, vlen).await?;
        self.write_from_iovecs(caller, &file, &iovecs, At::Position, RwfFlags(0))
            .await
    }

    pub(crate) async fn sys_pread(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        buf: UserSliceMut,
        pos: i64,
    ) -> SysResult<u32> {
        let file = self.file(caller.task(), fd)?;
        file.check_readable()?;
        file.check_seekable()?;
        let pos = u64::try_from(pos).map_err(|_| Errno::EINVAL)?;
        self.read_buf(caller, &file, buf, At::Offset(pos)).await
    }

    pub(crate) async fn sys_pwrite(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        buf: UserSlice,
        pos: i64,
    ) -> SysResult<u32> {
        let file = self.file(caller.task(), fd)?;
        file.check_writable()?;
        file.check_seekable()?;
        let pos = u64::try_from(pos).map_err(|_| Errno::EINVAL)?;
        self.write_buf(caller, &file, buf, At::Offset(pos)).await
    }

    /// `preadv2`; `preadv` is the same with no flags. A position of -1 uses the file position.
    pub(crate) async fn sys_preadv2(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        vec: ConstIovecArray,
        vlen: u32,
        pos: i64,
        flags: RwfFlags,
    ) -> SysResult<u32> {
        // `do_preadv`/`do_pwritev` check the offset first; -1 means the file position.
        if pos < -1 {
            return Err(Errno::EINVAL.into());
        }
        let file = self.file(caller.task(), fd)?;
        file.check_readable()?;
        let at = self.rw_position(&file, pos)?;
        rw_flags(flags)?;
        let iovecs = read_iovecs(caller.memory(), vec, vlen).await?;
        self.read_into_iovecs(caller, &file, &iovecs, at).await
    }

    pub(crate) async fn sys_pwritev2(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        vec: ConstIovecArray,
        vlen: u32,
        pos: i64,
        flags: RwfFlags,
    ) -> SysResult<u32> {
        // `do_preadv`/`do_pwritev` check the offset first; -1 means the file position.
        if pos < -1 {
            return Err(Errno::EINVAL.into());
        }
        let file = self.file(caller.task(), fd)?;
        file.check_writable()?;
        let at = self.rw_position(&file, pos)?;
        rw_flags(flags)?;
        let iovecs = read_iovecs(caller.memory(), vec, vlen).await?;
        self.write_from_iovecs(caller, &file, &iovecs, at, flags)
            .await
    }

    fn rw_position(&self, file: &OpenFile, pos: i64) -> Result<At, Errno> {
        if pos == -1 {
            return Ok(At::Position);
        }
        file.check_seekable()?;
        Ok(At::Offset(u64::try_from(pos).map_err(|_| Errno::EINVAL)?))
    }

    pub(crate) async fn sys_lseek(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        offset: i64,
        whence: SeekWhence,
    ) -> SysResult<i64> {
        let file = self.file(caller.task(), fd)?;
        let base = match whence {
            SeekWhence::SEEK_SET => SeekBase::Start,
            SeekWhence::SEEK_CUR => SeekBase::Current,
            SeekWhence::SEEK_END => SeekBase::End,
            SeekWhence::SEEK_DATA => SeekBase::Data,
            SeekWhence::SEEK_HOLE => SeekBase::Hole,
            _ => return Err(Errno::EINVAL.into()),
        };
        Ok(file.seek(base, offset).await? as i64)
    }

    /// `getdents64`: as many records as fit, resuming at the file position.
    pub(crate) async fn sys_getdents64(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        dirent: LinuxDirent64Ptr,
        count: u32,
    ) -> SysResult<i32> {
        let file = self.io_file(caller, fd)?;
        let inner = match &file.backing {
            Backing::Inode(inner) => inner,
            Backing::Stream(..) => return Err(Errno::ENOTDIR.into()),
        };
        let mut sink = Dirents {
            out: Vec::new(),
            limit: count as usize,
            too_small: false,
        };
        let _pos = file.lock_pos().await;
        let pos = inner.readdir(file.pos(), &mut sink).await?;
        if sink.out.is_empty() && sink.too_small {
            return Err(Errno::EINVAL.into());
        }
        caller
            .memory()
            .write(dirent.addr(), &sink.out)
            .await
            .map_err(|_| Errno::EFAULT)?;
        file.set_pos(pos);
        Ok(sink.out.len() as i32)
    }

    /// `sendfile`: from a file to anything writable, at `*offset` or the input's position, or
    /// from a socket without an offset. A byte stream with no reader left raises `SIGPIPE`.
    pub(crate) async fn sys_sendfile(
        &self,
        caller: &Caller<'_>,
        out_fd: Fd,
        in_fd: Fd,
        offset: LoffPtr,
        count: u32,
    ) -> SysResult<i32> {
        let input = self.file(caller.task(), in_fd)?;
        input.check_readable()?;
        // Only a file with a position takes an offset (`FMODE_PREAD`).
        let positional = input.check_seekable().is_ok();
        if !offset.is_null() && !positional {
            return Err(Errno::ESPIPE.into());
        }
        let output = self.file(caller.task(), out_fd)?;
        output.check_writable()?;
        // `do_splice_direct`: no appending output, and an input `splice_read` can read.
        if output.flags().contains(OpenFlags::O_APPEND) {
            return Err(Errno::EINVAL.into());
        }
        if !positional && !input.object().is_some_and(|object| object.splice_read()) {
            return Err(Errno::EINVAL.into());
        }
        let broken = Cell::new(false);
        let ret = self
            .sendfile(caller, &input, &output, offset, count, &broken)
            .await;
        self.epipe(caller, &output, ret, broken.get())
    }

    /// The transfer of `sys_sendfile`; `broken` as for [`OpenFile::write`].
    async fn sendfile(
        &self,
        caller: &Caller<'_>,
        input: &OpenFile,
        output: &OpenFile,
        offset: LoffPtr,
        count: u32,
        broken: &Cell<bool>,
    ) -> SysResult<i32> {
        let stream = input.check_seekable().is_err();
        let mut pos = match offset.is_null() {
            true => input.pos(),
            false => {
                let pos: i64 = read_value(caller.memory(), GuestPtr::new(offset.addr())).await?;
                u64::try_from(pos).map_err(|_| Errno::EINVAL)?
            }
        };
        let count = (count as usize).min(MAX_RW_COUNT);
        let interrupt = self.interrupt(caller);
        let mut done = 0;
        let mut buf = Scratch::new(count.min(IO_CHUNK));
        self.sync_file_mappings(caller, input).await;
        self.sync_file_mappings(caller, output).await;
        while done < count {
            let want = (count - done).min(buf.len());
            let read = match stream {
                true => {
                    input
                        .read(caller.task(), &*interrupt, &mut buf[..want])
                        .await
                }
                false => input.read_at(caller.task(), &mut buf[..want], pos).await,
            };
            let n = match read {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) if done > 0 => break,
                Err(err) => return Err(err),
            };
            let written = match output
                .write(caller.task(), &*interrupt, broken, &buf[..n])
                .await
            {
                Ok(written) => written,
                Err(_) if done > 0 => break,
                Err(err) => return Err(err),
            };
            pos += written as u64;
            done += written;
            if written < n {
                break;
            }
        }
        self.sync_file_mappings(caller, output).await;
        if !offset.is_null() {
            write_value(caller.memory(), offset, &(pos as i64)).await?;
        } else if !stream {
            input.set_pos(pos);
        }
        Ok(done as i32)
    }

    /// `copy_file_range` between regular files.
    #[allow(clippy::too_many_arguments)] // mirrors the syscall
    pub(crate) async fn sys_copy_file_range(
        &self,
        caller: &Caller<'_>,
        fd_in: Fd,
        off_in: LoffPtr,
        fd_out: Fd,
        off_out: LoffPtr,
        len: u32,
        flags: u32,
    ) -> SysResult<i32> {
        if flags != 0 {
            return Err(Errno::EINVAL.into());
        }
        let input = self.file(caller.task(), fd_in)?;
        let output = self.file(caller.task(), fd_out)?;
        input.check_readable()?;
        output.check_writable()?;
        if output.flags().contains(OpenFlags::O_APPEND) {
            return Err(Errno::EBADF.into());
        }
        let (Backing::Inode(src), Backing::Inode(dst)) = (&input.backing, &output.backing) else {
            return Err(Errno::EINVAL.into());
        };
        for file in [src, dst] {
            match file.file_type() {
                FileType::Regular => {}
                FileType::Directory => return Err(Errno::EISDIR.into()),
                _ => return Err(Errno::EINVAL.into()),
            }
        }
        let mut pos_in = read_offset(caller, off_in, &input).await?;
        let mut pos_out = read_offset(caller, off_out, &output).await?;
        // `generic_copy_file_checks`: offsets must not wrap; the copy stops at the input's end
        // and the largest file offset (EFBIG at it); only then may the ranges not overlap.
        let len = u64::from(len);
        if pos_in.checked_add(len).is_none() || pos_out.checked_add(len).is_none() {
            return Err(Errno::EOVERFLOW.into());
        }
        let size_in = src.node().getattr().await?.size;
        let len = len.min(size_in.saturating_sub(pos_in));
        let max_size = i64::MAX as u64;
        if pos_out >= max_size {
            return Err(Errno::EFBIG.into());
        }
        let len = len.min(max_size - pos_out);
        let same = src.lock_key() == dst.lock_key();
        if same && pos_in < pos_out + len && pos_out < pos_in + len {
            return Err(Errno::EINVAL.into());
        }
        let len = len.min(MAX_RW_COUNT as u64);
        let mut done = 0u64;
        let mut buf = Scratch::new((len as usize).min(IO_CHUNK));
        self.sync_file_mappings(caller, &input).await;
        self.sync_file_mappings(caller, &output).await;
        while done < len {
            let want = ((len - done) as usize).min(buf.len());
            let n = src.read_at(&mut buf[..want], pos_in).await?;
            if n == 0 {
                break;
            }
            let written = dst.write_at(&buf[..n], pos_out).await?;
            pos_in += written as u64;
            pos_out += written as u64;
            done += written as u64;
            if written < n {
                break;
            }
        }
        self.sync_file_mappings(caller, &output).await;
        if off_in.is_null() {
            input.set_pos(pos_in);
        } else {
            write_value(caller.memory(), off_in, &(pos_in as i64)).await?;
        }
        if off_out.is_null() {
            output.set_pos(pos_out);
        } else {
            write_value(caller.memory(), off_out, &(pos_out as i64)).await?;
        }
        Ok(done as i32)
    }

    /// `fsync`/`fdatasync`: streams have nothing to flush and say `EINVAL`.
    pub(crate) async fn sys_fsync(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        data_only: bool,
    ) -> SysResult<i32> {
        let file = self.io_file(caller, fd)?;
        match &file.backing {
            Backing::Inode(inner) => inner.sync(data_only).await?,
            Backing::Stream(..) => return Err(Errno::EINVAL.into()),
        }
        Ok(0)
    }

    pub(crate) async fn sys_syncfs(&self, caller: &Caller<'_>, fd: Fd) -> SysResult<i32> {
        let file = self.io_file(caller, fd)?;
        if let Backing::Inode(inner) = &file.backing {
            inner.path.dentry.sb().fs.sync().await?;
        }
        Ok(0)
    }

    /// `sync`: every mounted filesystem (`iterate_supers`); errors are not reported, as on
    /// Linux.
    pub(crate) async fn sys_sync(&self) -> SysResult<i32> {
        for sb in self.vfs.superblocks() {
            let _ = sb.fs.sync().await;
        }
        Ok(0)
    }

    pub(crate) async fn sys_sync_file_range(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        offset: i64,
        nbytes: i64,
        flags: SyncFileRangeFlags,
    ) -> SysResult<i32> {
        if !(flags & !SyncFileRangeFlags::SYNC_FILE_RANGE_WRITE_AND_WAIT).is_empty()
            || offset < 0
            || nbytes < 0
            || offset.checked_add(nbytes).is_none()
        {
            return Err(Errno::EINVAL.into());
        }
        let file = self.io_file(caller, fd)?;
        match &file.backing {
            Backing::Inode(inner)
                if matches!(
                    inner.file_type(),
                    FileType::Regular | FileType::Directory | FileType::BlockDevice
                ) =>
            {
                if flags.contains(SyncFileRangeFlags::SYNC_FILE_RANGE_WRITE) {
                    inner.sync(true).await?;
                }
                Ok(0)
            }
            _ => Err(Errno::ESPIPE.into()),
        }
    }

    /// `vfs_fallocate`.
    pub(crate) async fn sys_fallocate(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        mode: FallocFlags,
        offset: i64,
        len: i64,
    ) -> SysResult<i32> {
        if offset < 0 || len <= 0 {
            return Err(Errno::EINVAL.into());
        }
        let keep = mode.contains(FallocFlags::FALLOC_FL_KEEP_SIZE);
        let rest = mode & !FallocFlags::FALLOC_FL_KEEP_SIZE;
        let alloc = match rest {
            FallocFlags::FALLOC_FL_ALLOCATE_RANGE if keep => AllocMode::AllocateKeepSize,
            FallocFlags::FALLOC_FL_ALLOCATE_RANGE => AllocMode::Allocate,
            // Punching a hole must keep the size.
            FallocFlags::FALLOC_FL_PUNCH_HOLE if keep => AllocMode::PunchHole,
            FallocFlags::FALLOC_FL_PUNCH_HOLE => return Err(Errno::EOPNOTSUPP.into()),
            FallocFlags::FALLOC_FL_ZERO_RANGE => AllocMode::ZeroRange { keep_size: keep },
            _ => return Err(Errno::EOPNOTSUPP.into()),
        };
        let file = self.io_file(caller, fd)?;
        file.check_writable()?;
        let Backing::Inode(inner) = &file.backing else {
            return Err(Errno::ESPIPE.into());
        };
        match inner.file_type() {
            FileType::Regular => {}
            FileType::Directory => return Err(Errno::EISDIR.into()),
            FileType::Fifo => return Err(Errno::ESPIPE.into()),
            _ => return Err(Errno::ENODEV.into()),
        }
        if offset.checked_add(len).is_none() {
            return Err(Errno::EFBIG.into());
        }
        if inner.path.mnt.is_read_only() {
            return Err(Errno::EROFS.into());
        }
        // Linux refuses all but plain allocation on an append-only inode (`IS_APPEND`, from
        // `chattr +a`), which no filesystem here has; an `O_APPEND` descriptor is no such thing.
        self.sync_file_mappings(caller, &file).await;
        let allocated = inner.allocate(alloc, offset as u64, len as u64).await;
        self.sync_file_mappings(caller, &file).await;
        allocated?;
        Ok(0)
    }

    /// `do_ftruncate`.
    pub(crate) async fn sys_ftruncate(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        length: i64,
    ) -> SysResult<i32> {
        if length < 0 {
            return Err(Errno::EINVAL.into());
        }
        let file = self.io_file(caller, fd)?;
        let Backing::Inode(inner) = &file.backing else {
            return Err(Errno::EINVAL.into());
        };
        if inner.file_type() != FileType::Regular || !inner.writable {
            return Err(Errno::EINVAL.into());
        }
        // `security_file_truncate`.
        if !file.landlock_allows(crate::landlock::TRUNCATE) {
            return Err(Errno::EACCES.into());
        }
        self.sync_file_mappings(caller, &file).await;
        let resized = inner.set_size(length as u64).await;
        self.sync_file_mappings(caller, &file).await;
        resized?;
        Ok(0)
    }

    /// `fadvise64_64`: validated, then ignored; there is no page cache to advise.
    pub(crate) fn sys_fadvise(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        len: i64,
        advice: FadviseAdvice,
    ) -> SysResult<i32> {
        let file = self.file(caller.task(), fd)?;
        if matches!(file.backing, Backing::Stream(..)) {
            return Err(Errno::ESPIPE.into());
        }
        if len < 0 || advice.0 > 5 {
            return Err(Errno::EINVAL.into());
        }
        Ok(0)
    }

    pub(crate) fn sys_readahead(&self, caller: &Caller<'_>, fd: Fd) -> SysResult<i32> {
        let file = self.io_file(caller, fd)?;
        file.check_readable()?;
        match &file.backing {
            Backing::Inode(inner) if inner.file_type() == FileType::Regular => Ok(0),
            _ => Err(Errno::EINVAL.into()),
        }
    }
}

/// `*ptr`, or the file position for a NULL `ptr`.
async fn read_offset(caller: &Caller<'_>, ptr: LoffPtr, file: &OpenFile) -> Result<u64, Errno> {
    if ptr.is_null() {
        return Ok(file.pos());
    }
    let pos: i64 = read_value(caller.memory(), GuestPtr::new(ptr.addr()))
        .await
        .map_err(|_| Errno::EFAULT)?;
    u64::try_from(pos).map_err(|_| Errno::EINVAL)
}

fn rw_flags(flags: RwfFlags) -> Result<(), Errno> {
    if flags.0 & !RWF_KNOWN != 0 {
        return Err(Errno::EOPNOTSUPP);
    }
    Ok(())
}

/// Packs `linux_dirent64` records: the header, the name and a NUL, padded to 8 bytes.
struct Dirents {
    out: Vec<u8>,
    limit: usize,
    too_small: bool,
}

impl DirSink for Dirents {
    fn push(&mut self, entry: DirEntry<'_>) -> bool {
        const NAME_OFFSET: usize = 19;
        let reclen = (NAME_OFFSET + entry.name.len() + 1).next_multiple_of(8);
        if self.out.len() + reclen > self.limit {
            self.too_small = true;
            return false;
        }
        let header = LinuxDirent64 {
            d_ino: entry.ino,
            d_off: entry.next as i64,
            d_reclen: reclen as u16,
            d_type: entry.file_type.dirent_type(),
        };
        let start = self.out.len();
        self.out.resize(start + reclen, 0);
        let record = &mut self.out[start..];
        let mut head = [0; LinuxDirent64::SIZE];
        header.write_to(&mut head);
        record[..NAME_OFFSET].copy_from_slice(&head[..NAME_OFFSET]);
        record[NAME_OFFSET..][..entry.name.len()].copy_from_slice(entry.name);
        true
    }
}

pub(crate) async fn read_iovecs(
    memory: &dyn UserMemory,
    vec: ConstIovecArray,
    vlen: u32,
) -> Result<Vec<Iovec>, Errno> {
    if vlen > IOV_MAX {
        return Err(Errno::EINVAL);
    }
    let mut raw = vec![0; vlen as usize * Iovec::SIZE];
    memory
        .read(vec.addr(), &mut raw)
        .await
        .map_err(|_| Errno::EFAULT)?;
    let iovecs: Vec<Iovec> = raw
        .chunks_exact(Iovec::SIZE)
        .filter_map(Iovec::read_from)
        .collect();
    // `iovec_from_user`: a negative length is `EINVAL`; the total is cut to `MAX_RW_COUNT`.
    if iovecs.iter().any(|iov| iov.iov_len > i32::MAX as u32) {
        return Err(Errno::EINVAL);
    }
    let mut left = MAX_RW_COUNT as u32;
    Ok(iovecs
        .into_iter()
        .map(|iov| {
            let iov_len = iov.iov_len.min(left);
            left -= iov_len;
            Iovec { iov_len, ..iov }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_reuse_the_thread_scratch_without_zeroing_it_again() {
        let mut first = Scratch::new(IO_CHUNK);
        first.fill(7);
        let at = first.as_ptr();
        drop(first);

        // The same buffer comes back, with what the last call left in it.
        let second = Scratch::new(16);
        assert_eq!(second.as_ptr(), at);
        assert_eq!(second.len(), 16);
        assert_eq!(second[..], [7; 16]);
        // While it is taken, a call suspended meanwhile gets a buffer of its own.
        let other = Scratch::new(16);
        assert_ne!(other.as_ptr(), at);
        assert_eq!(other[..], [0; 16]);
        drop(second);
        drop(other);
        // The larger buffer stays, and a datagram-sized one does not displace it.
        drop(Scratch::new(IO_CHUNK + 1));
        assert_eq!(Scratch::new(1).as_ptr(), at);
    }
}
