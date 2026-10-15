//! File syscalls: opening and removing paths, and I/O on open files.

use std::sync::Arc;

use wasixv2_abi::{
    AtFd, AtFlags, ConstIovecArray, Cstr, FcntlCmd, FdFlags, GuestLayout, Iovec, OpenFlags,
};
use wasixv2_syscalls::{
    Caller, CstrError, Errno, Fd, SysResult, UserMemory, UserSlice, UserSliceMut, read_cstr,
};
use wasixv2_vfs::{FileType, Inode, PATH_MAX, walk_parent};

use crate::{
    IO_CHUNK, Kernel,
    file::{Backing, OpenFile, SeekBase},
};

/// Most iovecs one `readv` or `writev` takes, as Linux's `UIO_MAXIOV`.
const IOV_MAX: u32 = 1024;

impl Kernel {
    pub(crate) async fn open(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        path: Cstr,
        flags: OpenFlags,
    ) -> SysResult<Fd> {
        let path = read_path(caller.memory(), path).await?;
        let parent = walk_parent(&self.root, &self.start_dir(caller, dfd, &path)?, &path)?;
        let creating = flags.contains(OpenFlags::O_CREAT);
        let inode = match parent.name {
            None if creating => return Err(Errno::EISDIR.into()),
            None => parent.dir,
            Some(name) => match parent.dir.lookup(&name) {
                Ok(_) if creating && flags.contains(OpenFlags::O_EXCL) => {
                    return Err(Errno::EEXIST.into());
                }
                Ok(inode) => inode,
                Err(Errno::ENOENT) if creating => parent.dir.create(&name, FileType::Regular)?,
                Err(errno) => return Err(errno.into()),
            },
        };

        let access = flags & OpenFlags::O_ACCMODE;
        match inode.file_type() {
            FileType::Directory if creating || access != OpenFlags::O_RDONLY => {
                return Err(Errno::EISDIR.into());
            }
            FileType::Regular if flags.contains(OpenFlags::O_DIRECTORY) => {
                return Err(Errno::ENOTDIR.into());
            }
            FileType::Regular
                if flags.contains(OpenFlags::O_TRUNC) && access != OpenFlags::O_RDONLY =>
            {
                inode.truncate(0)?;
            }
            _ => {}
        }

        let file = Arc::new(OpenFile::new(Backing::Inode(inode), flags));
        let cloexec = flags.contains(OpenFlags::O_CLOEXEC);
        Ok(self
            .files(caller.task())
            .lock()
            .unwrap()
            .install(file, cloexec))
    }

    pub(crate) async fn unlink(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        path: Cstr,
        flags: AtFlags,
    ) -> SysResult<i32> {
        if !(flags & !AtFlags::AT_REMOVEDIR).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        let path = read_path(caller.memory(), path).await?;
        let parent = walk_parent(&self.root, &self.start_dir(caller, dfd, &path)?, &path)?;
        let removing_dir = flags.contains(AtFlags::AT_REMOVEDIR);
        match (parent.name, removing_dir) {
            (Some(name), true) => parent.dir.remove(&name, FileType::Directory)?,
            (Some(name), false) => parent.dir.remove(&name, FileType::Regular)?,
            // `rmdir("dir/.")` and friends.
            (None, true) => return Err(Errno::EINVAL.into()),
            (None, false) => return Err(Errno::EISDIR.into()),
        }
        Ok(0)
    }

    /// Where a relative path starts: the working directory (always `/` for now) or `dfd`.
    fn start_dir(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        path: &[u8],
    ) -> Result<Arc<dyn Inode>, Errno> {
        if path.starts_with(b"/") || dfd.0 == AtFd::AT_FDCWD.0 {
            return Ok(self.root.clone());
        }
        match &self.file(caller.task(), dfd)?.backing {
            Backing::Inode(inode) if inode.file_type() == FileType::Directory => Ok(inode.clone()),
            _ => Err(Errno::ENOTDIR),
        }
    }

    pub(crate) fn close_fd(&self, caller: &Caller<'_>, fd: Fd) -> SysResult<i32> {
        let files = self.files(caller.task());
        let file = files.lock().unwrap().remove(fd);
        file.map(|_| 0).ok_or(Errno::EBADF.into())
    }

    pub(crate) async fn read_vectored(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        vec: ConstIovecArray,
        vlen: u32,
    ) -> SysResult<u32> {
        let file = self.file(caller.task(), fd)?;
        file.check_readable()?;
        let iovecs = read_iovecs(caller.memory(), vec, vlen).await?;
        let total: usize = iovecs.iter().map(|iov| iov.iov_len as usize).sum();
        let mut data = vec![0; total.min(IO_CHUNK)];
        let n = file.read(&mut data).await?;
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
    pub(crate) async fn write_vectored(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        vec: ConstIovecArray,
        vlen: u32,
    ) -> SysResult<u32> {
        let file = self.file(caller.task(), fd)?;
        file.check_writable()?;
        let mut data = Vec::new();
        for iov in read_iovecs(caller.memory(), vec, vlen).await? {
            let slice = UserSlice::new(iov.iov_base, iov.iov_len);
            data.extend(
                slice
                    .copy_in(caller.memory(), IO_CHUNK - data.len())
                    .await?,
            );
        }
        Ok(file.write(&data).await? as u32)
    }

    pub(crate) async fn pread(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        buf: UserSliceMut,
        pos: i64,
    ) -> SysResult<u32> {
        let pos = u64::try_from(pos).map_err(|_| Errno::EINVAL)?;
        let file = self.file(caller.task(), fd)?;
        file.check_seekable()?;
        file.check_readable()?;
        let mut data = vec![0; (buf.len as usize).min(IO_CHUNK)];
        let n = file.read_at(&mut data, pos).await?;
        buf.copy_out(caller.memory(), &data[..n]).await?;
        Ok(n as u32)
    }

    pub(crate) async fn pwrite(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        buf: UserSlice,
        pos: i64,
    ) -> SysResult<u32> {
        let pos = u64::try_from(pos).map_err(|_| Errno::EINVAL)?;
        let file = self.file(caller.task(), fd)?;
        file.check_seekable()?;
        file.check_writable()?;
        let data = buf.copy_in(caller.memory(), IO_CHUNK).await?;
        Ok(file.write_at(&data, pos).await? as u32)
    }

    pub(crate) fn seek(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        offset: i64,
        whence: wasixv2_abi::SeekWhence,
    ) -> Result<u64, Errno> {
        use wasixv2_abi::SeekWhence;

        let file = self.file(caller.task(), fd)?;
        let base = match whence {
            SeekWhence::SEEK_SET => SeekBase::Start,
            SeekWhence::SEEK_CUR => SeekBase::Current,
            SeekWhence::SEEK_END => SeekBase::End,
            _ => return Err(Errno::EINVAL),
        };
        file.seek(base, offset)
    }

    /// The descriptor and file status flag commands; locks, leases, pipes and seals are missing.
    pub(crate) fn fcntl(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        cmd: FcntlCmd,
        arg: u32,
    ) -> SysResult<i32> {
        let files = self.files(caller.task());
        let mut files = files.lock().unwrap();
        let file = files.get(fd).ok_or(Errno::EBADF)?;
        match cmd {
            FcntlCmd::F_GETFD => {
                let cloexec = files.cloexec(fd).unwrap_or_default();
                Ok(if cloexec {
                    FdFlags::FD_CLOEXEC.0 as i32
                } else {
                    0
                })
            }
            FcntlCmd::F_SETFD => {
                files.set_cloexec(fd, arg & FdFlags::FD_CLOEXEC.0 != 0);
                Ok(0)
            }
            FcntlCmd::F_GETFL => Ok(file.flags().0 as i32),
            FcntlCmd::F_SETFL => {
                file.set_status_flags(OpenFlags(arg));
                Ok(0)
            }
            FcntlCmd::F_GETPIPE_SZ | FcntlCmd::F_SETPIPE_SZ => crate::pipe::fcntl(&file, cmd, arg),
            _ => Err(Errno::EINVAL.into()),
        }
    }
}

async fn read_path(memory: &dyn UserMemory, path: Cstr) -> Result<Vec<u8>, Errno> {
    read_cstr(memory, path, PATH_MAX)
        .await
        .map_err(|err| match err {
            CstrError::Fault => Errno::EFAULT,
            CstrError::TooLong => Errno::ENAMETOOLONG,
        })
}

async fn read_iovecs(
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
