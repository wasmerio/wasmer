//! Data syscalls on open files (`fs/read_write.c`, `fs/readdir.c`, `fs/sync.c`, `fs/open.c`).

use wasixv2_abi::{
    ConstIovecArray, FadviseAdvice, FallocFlags, GuestLayout, GuestPtr, Iovec, LinuxDirent64,
    LinuxDirent64Ptr, LoffPtr, OpenFlags, RwfFlags, SeekWhence, SyncFileRangeFlags,
};
use wasixv2_syscalls::{
    Caller, Errno, Fd, SysResult, UserMemory, UserSlice, UserSliceMut, read_value, write_value,
};
use wasixv2_vfs::{AllocMode, DirEntry, DirSink, FileType};

use crate::{
    IO_CHUNK, Kernel,
    file::{Backing, OpenFile, SeekBase},
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
enum At {
    /// The file position, which moves.
    Position,
    /// An explicit offset; the position stays.
    Offset(u64),
}

impl Kernel {
    async fn read_into_iovecs(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        iovecs: &[Iovec],
        at: At,
    ) -> SysResult<u32> {
        let total: usize = iovecs.iter().map(|iov| iov.iov_len as usize).sum();
        let mut data = vec![0; total.min(IO_CHUNK)];
        let n = match at {
            At::Position => file.read(&mut data).await?,
            At::Offset(offset) => file.read_at(&mut data, offset).await?,
        };
        let mut rest = &data[..n];
        for iov in iovecs {
            let (chunk, tail) = rest.split_at(rest.len().min(iov.iov_len as usize));
            UserSliceMut::new(iov.iov_base, iov.iov_len)
                .copy_out(caller.memory(), chunk)
                .await?;
            rest = tail;
        }
        Ok(n as u32)
    }

    /// Gathers the buffers into one write, so they reach the file together.
    async fn write_from_iovecs(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        iovecs: &[Iovec],
        at: At,
    ) -> SysResult<u32> {
        let mut data = Vec::new();
        for iov in iovecs {
            let slice = UserSlice::new(iov.iov_base, iov.iov_len);
            data.extend(
                slice
                    .copy_in(caller.memory(), IO_CHUNK - data.len())
                    .await?,
            );
        }
        let n = match at {
            At::Position => file.write(&data).await?,
            At::Offset(offset) => file.write_at(&data, offset).await?,
        };
        Ok(n as u32)
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
        self.write_from_iovecs(caller, &file, &iovecs, At::Position)
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
        let mut data = vec![0; (buf.len as usize).min(IO_CHUNK)];
        let n = file.read_at(&mut data, pos).await?;
        buf.copy_out(caller.memory(), &data[..n]).await?;
        Ok(n as u32)
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
        let data = buf.copy_in(caller.memory(), IO_CHUNK).await?;
        Ok(file.write_at(&data, pos).await? as u32)
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
        let file = self.file(caller.task(), fd)?;
        file.check_writable()?;
        let at = self.rw_position(&file, pos)?;
        rw_flags(flags)?;
        let iovecs = read_iovecs(caller.memory(), vec, vlen).await?;
        let mut data = Vec::new();
        for iov in &iovecs {
            let slice = UserSlice::new(iov.iov_base, iov.iov_len);
            data.extend(
                slice
                    .copy_in(caller.memory(), IO_CHUNK - data.len())
                    .await?,
            );
        }
        let n = match &file.backing {
            Backing::Inode(inner) if flags.contains(RwfFlags::RWF_APPEND) => {
                let (end, n) = inner.append(&data).await?;
                if matches!(at, At::Position) {
                    file.set_pos(end);
                }
                n
            }
            Backing::Inode(inner)
                if flags.contains(RwfFlags::RWF_NOAPPEND)
                    && file.flags().contains(OpenFlags::O_APPEND) =>
            {
                let offset = match at {
                    At::Offset(offset) => offset,
                    At::Position => file.pos(),
                };
                let n = inner.write_at(&data, offset).await?;
                if matches!(at, At::Position) {
                    file.set_pos(offset + n as u64);
                }
                n
            }
            _ => match at {
                At::Position => file.write(&data).await?,
                At::Offset(offset) => file.write_at(&data, offset).await?,
            },
        };
        if flags.intersects(RwfFlags::RWF_DSYNC | RwfFlags::RWF_SYNC)
            && let Backing::Inode(inner) = &file.backing
        {
            inner.sync(!flags.contains(RwfFlags::RWF_SYNC)).await?;
        }
        Ok(n as u32)
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
            Backing::Stream(_) => return Err(Errno::ENOTDIR.into()),
        };
        let mut sink = Dirents {
            out: Vec::new(),
            limit: count as usize,
            too_small: false,
        };
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
        let count = (count as usize).min(i32::MAX as usize);
        let mut done = 0;
        let mut buf = vec![0; count.min(IO_CHUNK)];
        while done < count {
            let want = (count - done).min(buf.len());
            let n = input.read_at(&mut buf[..want], pos).await?;
            if n == 0 {
                break;
            }
            let written = match output.write(&buf[..n]).await {
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
        let len = len as u64;
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
            Backing::Stream(_) => return Err(Errno::EINVAL.into()),
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
        self.vfs.set_size(&inner.path, length as u64).await?;
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
        if matches!(file.backing, Backing::Stream(_)) {
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
    // Linux rejects a total that overflows `ssize_t`.
    let total: u64 = iovecs.iter().map(|iov| u64::from(iov.iov_len)).sum();
    if total > i32::MAX as u64 {
        return Err(Errno::EINVAL);
    }
    Ok(iovecs)
}
