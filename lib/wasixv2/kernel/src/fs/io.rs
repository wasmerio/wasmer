//! Data syscalls on open files (`fs/read_write.c`, `fs/readdir.c`, `fs/sync.c`, `fs/open.c`).

use wasixv2_abi::{
    ConstIovecArray, FadviseAdvice, FallocFlags, GuestLayout, GuestPtr, Iovec, LinuxDirent64,
    LinuxDirent64Ptr, LoffPtr, OpenFlags, RwfFlags, SeekWhence, SyncFileRangeFlags,
};
use wasixv2_syscalls::{
    Caller, Errno, Fd, SysResult, UserMemory, UserSlice, UserSliceMut, read_value, write_value,
};
use wasixv2_vfs::{AllocMode, DirEntry, DirSink, FileType};

use wasixv2_wait::Interrupt;

use crate::{
    IO_CHUNK, Kernel, MAX_RW_COUNT,
    file::{Backing, OpenFile, SeekBase, check_file_size},
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

/// A position in a list of guest buffers, for transfers made in steps.
struct Cursor<'a> {
    iovecs: &'a [Iovec],
    index: usize,
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(iovecs: &'a [Iovec]) -> Self {
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
    async fn copy_in(&mut self, memory: &dyn UserMemory, buf: &mut [u8]) -> (usize, bool) {
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

/// Where the steps of one read or write go. A transfer at the file position holds the
/// position lock throughout (`fdget_pos`), so threads sharing the description never interleave
/// within one call.
struct Target<'a> {
    file: &'a OpenFile,
    interrupt: &'a dyn Interrupt,
    at: At,
    /// Writes go to the end of the file (`O_APPEND` or `RWF_APPEND`, unless `RWF_NOAPPEND`).
    append: bool,
    /// The offset the transfer started at, for files.
    start: u64,
}

impl Target<'_> {
    async fn read(&self, buf: &mut [u8], done: usize) -> SysResult<usize> {
        let Backing::Inode(inner) = &self.file.backing else {
            return self.file.read(self.interrupt, buf).await;
        };
        let offset = self.start + done as u64;
        let n = inner.read_at(buf, offset).await?;
        if matches!(self.at, At::Position) {
            self.file.set_pos(offset + n as u64);
        }
        Ok(n)
    }

    async fn write(&self, data: &[u8], done: usize) -> SysResult<usize> {
        let Backing::Inode(inner) = &self.file.backing else {
            return self.file.write(self.interrupt, data).await;
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
    /// the file ends; a stream read returns what one read of the object gives, as Linux's
    /// `pipe_read` and socket reads do. A blocked read ends on a signal.
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
            file,
            interrupt: &*interrupt,
            at,
            append: false,
            start: match at {
                At::Position => file.pos(),
                At::Offset(offset) => offset,
            },
        };
        let stream = file.object().is_some();
        let mut buf = vec![0; total.min(IO_CHUNK)];
        let mut cursor = Cursor::new(iovecs);
        let mut done = 0;
        loop {
            let want = (total - done).min(buf.len());
            let n = match target.read(&mut buf[..want], done).await {
                Ok(n) => n,
                Err(_) if done > 0 => break,
                Err(err) => return Err(err),
            };
            let (copied, faulted) = cursor.copy_out(caller.memory(), &buf[..n]).await;
            if copied == 0 && n > 0 && done == 0 {
                return Err(Errno::EFAULT.into());
            }
            done += copied;
            if stream || faulted || n < want || done == total {
                break;
            }
        }
        Ok(done as u32)
    }

    /// Writes the buffers in order, in steps, until all is written or an object takes less:
    /// a blocking write to a stream writes everything, as Linux's do, while a signal, a full
    /// non-blocking object or a fault ends it with the count so far. A datagram is one write.
    async fn write_from_iovecs(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        iovecs: &[Iovec],
        at: At,
        flags: RwfFlags,
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
            file,
            interrupt: &*interrupt,
            at,
            append: flags.contains(RwfFlags::RWF_APPEND)
                || (o_append && !flags.contains(RwfFlags::RWF_NOAPPEND)),
            start: match at {
                At::Position => file.pos(),
                At::Offset(offset) => offset,
            },
        };
        let step = if messages { total } else { total.min(IO_CHUNK) };
        let mut buf = vec![0; step];
        let mut cursor = Cursor::new(iovecs);
        let mut done = 0;
        loop {
            let (n, faulted) = cursor.copy_in(caller.memory(), &mut buf).await;
            if n == 0 && faulted {
                return match done {
                    0 => Err(Errno::EFAULT.into()),
                    _ => Ok(done as u32),
                };
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
        if flags.intersects(RwfFlags::RWF_DSYNC | RwfFlags::RWF_SYNC)
            && let Backing::Inode(inner) = &file.backing
        {
            inner.sync(!flags.contains(RwfFlags::RWF_SYNC)).await?;
        }
        Ok(done as u32)
    }

    /// `write(2)` on `file`, from one guest buffer. `EPIPE` on a byte stream raises
    /// `SIGPIPE`.
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
        let ret = self
            .write_from_iovecs(caller, file, &iovec, at, RwfFlags(0))
            .await;
        self.epipe(caller, file, ret)
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

    /// `SIGPIPE` for an `EPIPE` from a byte stream; datagram and seqpacket sockets only fail.
    pub(crate) fn epipe<T>(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        ret: SysResult<T>,
    ) -> SysResult<T> {
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
        let ret = self
            .write_from_iovecs(caller, &file, &iovecs, At::Position, RwfFlags(0))
            .await;
        self.epipe(caller, &file, ret)
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
        let ret = self
            .write_from_iovecs(caller, &file, &iovecs, at, flags)
            .await;
        self.epipe(caller, &file, ret)
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

    /// `sendfile`: from a file to anything writable, at `*offset` or the input's position.
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
        let output = self.file(caller.task(), out_fd)?;
        output.check_writable()?;
        if output.flags().contains(OpenFlags::O_APPEND) {
            return Err(Errno::EINVAL.into());
        }
        input.check_seekable().map_err(|_| Errno::EINVAL)?;
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
        let mut buf = vec![0; count.min(IO_CHUNK)];
        while done < count {
            let want = (count - done).min(buf.len());
            let n = input.read_at(&mut buf[..want], pos).await?;
            if n == 0 {
                break;
            }
            let written = match output.write(&*interrupt, &buf[..n]).await {
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
        if offset.is_null() {
            input.set_pos(pos);
        } else {
            write_value(caller.memory(), offset, &(pos as i64)).await?;
        }
        Ok(done as i32)
    }

    /// `sendfile`, raising `SIGPIPE` when the output is a byte stream with no reader left.
    pub(crate) async fn sys_sendfile_signalled(
        &self,
        caller: &Caller<'_>,
        out_fd: Fd,
        in_fd: Fd,
        offset: LoffPtr,
        count: u32,
    ) -> SysResult<i32> {
        let ret = self
            .sys_sendfile(caller, out_fd, in_fd, offset, count)
            .await;
        match self.file(caller.task(), out_fd) {
            Ok(output) => self.epipe(caller, &output, ret),
            Err(_) => ret,
        }
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
        let len = u64::from(len).min(MAX_RW_COUNT as u64);
        let same = src.lock_key() == dst.lock_key();
        if same && pos_in < pos_out + len && pos_out < pos_in + len {
            return Err(Errno::EINVAL.into());
        }
        let mut done = 0u64;
        let mut buf = vec![0; (len as usize).min(IO_CHUNK)];
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

    /// `sync`: every filesystem in the tree; errors are not reported, as on Linux.
    pub(crate) async fn sys_sync(&self) -> SysResult<i32> {
        for mount in self.vfs.namespace().mounts() {
            let _ = mount.sb().fs.sync().await;
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
        if file.flags().contains(OpenFlags::O_APPEND)
            && !matches!(alloc, AllocMode::Allocate | AllocMode::AllocateKeepSize)
        {
            return Err(Errno::EPERM.into());
        }
        inner.allocate(alloc, offset as u64, len as u64).await?;
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
        inner.set_size(length as u64).await?;
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
