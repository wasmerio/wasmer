//! Descriptor calls (`fd_*` apart from reading, writing, closing and directories), and what the
//! path and directory calls share: path arguments, `statx` to v1 `filestat`, file types.
//!
//! Rights are reported, never enforced: `fd_fdstat_get` derives them from the open file's
//! access mode and type, every descriptor may hand on every right, and access is what Linux
//! decides for the open file.

use std::{collections::HashSet, sync::Mutex};

use wasixv2_abi::{
    AtFlags, ConstIovecArray, FadviseAdvice, FallocFlags, FcntlCmd, Fd, FdFlags, FileMode,
    GuestPtr, GuestPtrMut, IoctlRequest, KernelTimespec, OpenFlags, SeekWhence, SocketLevel,
    SocketOption, SocketType, Statx, StatxMask, StatxTimestamp, UtimeSpecial,
};
use wasixv2_syscalls::{Control, SysErr, Syscalls, UserMemory};
use wasmer::RuntimeError;

use crate::{
    abi::{
        Errno,
        fs::{
            Advice, Fdflags, Fdflagsext, Fdstat, Filestat, Filetype, Fstflags, Rights,
            Snapshot0Filestat, Snapshot0Whence, Whence,
        },
    },
    errno::V1Result,
    shim::{self, Env, Frame},
};

/// Per-process filesystem compat state.
#[derive(Debug, Default)]
pub struct State {
    /// Preopened descriptors the guest renumbered another file onto: they are ordinary
    /// descriptors from then on.
    replaced: Mutex<HashSet<i32>>,
}

/// Whether `fd` still is one of the process's preopened directories. v1 kept them for the
/// life of the process: wasix-libc routes paths to them, and closing one succeeds but leaves
/// it open.
pub(crate) async fn is_preopen<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    fd: i32,
) -> V1Result<bool> {
    let startup = frame.startup().await?;
    if startup.preopen(Fd(fd)).is_none() {
        return Ok(false);
    }
    let proc = frame.proc().await?;
    Ok(!proc.fs.replaced.lock().unwrap().contains(&fd))
}

/// Linux's `PATH_MAX`: longer v1 paths fail before they are copied.
const PATH_MAX: u32 = 4096;

/// The result of a v2 call whose errno is an answer rather than a failure, such as a probe.
/// Interruptions still end the body.
pub(crate) fn answer<T>(ret: Result<T, SysErr>) -> V1Result<Result<T, wasixv2_abi::Errno>> {
    match ret {
        Ok(value) => Ok(Ok(value)),
        Err(SysErr::Errno(errno)) if errno != wasixv2_abi::Errno::EINTR => Ok(Err(errno)),
        Err(err) => Err(err.into()),
    }
}

/// The guest path `(ptr, len)` as a C string in scratch, for the kernel's `*at` calls.
pub(crate) async fn path_arg<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    ptr: i32,
    len: i32,
) -> V1Result<wasixv2_abi::Cstr> {
    let path = guest_path(frame, ptr, len).await?;
    Ok(frame.mem.alloc_cstr(&path)?)
}

/// The bytes of the guest path `(ptr, len)`. v1 passes lengths, Linux NUL-terminated strings:
/// a path with a NUL in it cannot be expressed and is `INVAL`.
async fn guest_path<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    ptr: i32,
    len: i32,
) -> V1Result<Vec<u8>> {
    let len = len as u32;
    if len >= PATH_MAX {
        return shim::fail(Errno::NAMETOOLONG);
    }
    let mut path = vec![0; len as usize];
    frame.mem.guest().read(ptr as u32, &mut path).await?;
    if path.contains(&0) {
        return shim::fail(Errno::INVAL);
    }
    Ok(path)
}

/// `statx` of the open file `fd`.
pub(crate) async fn fstat<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    fd: Fd,
    mask: StatxMask,
) -> V1Result<Statx> {
    let empty = frame.mem.alloc_cstr(b"")?;
    stat_at(frame, fd, empty, AtFlags::AT_EMPTY_PATH, mask).await
}

/// `statx` of `path` relative to `dirfd`.
pub(crate) async fn stat_at<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    dirfd: Fd,
    path: wasixv2_abi::Cstr,
    flags: AtFlags,
    mask: StatxMask,
) -> V1Result<Statx> {
    let buf = frame.mem.alloc_mut::<Statx>()?;
    frame
        .syscalls
        .statx(&frame.caller(), dirfd, path, flags, mask, buf)
        .await?;
    Ok(frame.mem.take(buf))
}

/// The v1 type of a file of Linux mode `mode`, as far as the mode tells. FIFOs have no v1
/// type, and v1 reported pipes as `UNKNOWN`.
pub(crate) fn filetype_of_mode(mode: u32) -> Filetype {
    let kind = mode & FileMode::S_IFMT.0;
    [
        (FileMode::S_IFREG, Filetype::REGULAR_FILE),
        (FileMode::S_IFDIR, Filetype::DIRECTORY),
        (FileMode::S_IFLNK, Filetype::SYMBOLIC_LINK),
        (FileMode::S_IFCHR, Filetype::CHARACTER_DEVICE),
        (FileMode::S_IFBLK, Filetype::BLOCK_DEVICE),
        (FileMode::S_IFSOCK, Filetype::SOCKET_STREAM),
    ]
    .into_iter()
    .find(|(mode, _)| mode.0 == kind)
    .map_or(Filetype::UNKNOWN, |(_, filetype)| filetype)
}

/// The v1 type of a socket of Linux type `ty`.
fn socket_filetype(ty: SocketType) -> Filetype {
    match ty {
        SocketType::SOCK_STREAM => Filetype::SOCKET_STREAM,
        SocketType::SOCK_DGRAM => Filetype::SOCKET_DGRAM,
        SocketType::SOCK_RAW => Filetype::SOCKET_RAW,
        SocketType::SOCK_SEQPACKET => Filetype::SOCKET_SEQPACKET,
        _ => Filetype::UNKNOWN,
    }
}

/// The v1 type of the open file `fd` whose mode is `mode`. v1 tells socket types apart, so
/// a descriptor that may be a socket is asked for `SO_TYPE`.
pub(crate) async fn fd_filetype<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    fd: Fd,
    mode: u32,
) -> V1Result<Filetype> {
    let kind = mode & FileMode::S_IFMT.0;
    if kind != FileMode::S_IFSOCK.0 && kind != FileMode::S_IFIFO.0 {
        return Ok(filetype_of_mode(mode));
    }
    let value = frame.mem.alloc_mut::<i32>()?;
    let len = frame.mem.alloc(&4u32)?;
    let ret = frame
        .syscalls
        .getsockopt(
            &frame.caller(),
            fd,
            SocketLevel::SOL_SOCKET,
            SocketOption::SO_TYPE.0,
            GuestPtrMut::new(value.addr()),
            GuestPtrMut::new(len.addr()),
        )
        .await;
    Ok(match answer(ret)? {
        Ok(_) => socket_filetype(SocketType(frame.mem.take(value))),
        Err(_) => filetype_of_mode(mode),
    })
}

/// Whether `fd` is a terminal (`TCGETS` succeeds).
async fn is_tty<S: Syscalls + Control + 'static>(frame: &Frame<'_, S>, fd: Fd) -> V1Result<bool> {
    // Room for the kernel's `struct termios`.
    let termios = frame.mem.alloc_zeroed(64)?;
    let ret = frame
        .syscalls
        .ioctl(&frame.caller(), fd, IoctlRequest::TCGETS, termios.addr)
        .await;
    Ok(answer(ret)?.is_ok())
}

/// The v1 timestamp (ns since the epoch, unsigned) of a Linux one.
pub(crate) fn timestamp(ts: StatxTimestamp) -> u64 {
    u64::try_from(ts.tv_sec)
        .unwrap_or(0)
        .saturating_mul(1_000_000_000)
        .saturating_add(u64::from(ts.tv_nsec))
}

/// `makedev` as musl encodes `st_dev`.
fn makedev(major: u32, minor: u32) -> u64 {
    let (major, minor) = (u64::from(major), u64::from(minor));
    ((major & 0xffff_f000) << 32)
        | ((major & 0x0000_0fff) << 8)
        | ((minor & 0xffff_ff00) << 12)
        | (minor & 0x0000_00ff)
}

/// The v1 `filestat` of `st`, whose type is `filetype`.
pub(crate) fn filestat(st: &Statx, filetype: Filetype) -> Filestat {
    Filestat {
        st_dev: makedev(st.stx_dev_major, st.stx_dev_minor),
        st_ino: st.stx_ino,
        st_filetype: filetype,
        st_nlink: u64::from(st.stx_nlink),
        st_size: st.stx_size,
        st_atim: timestamp(st.stx_atime),
        st_mtim: timestamp(st.stx_mtime),
        st_ctim: timestamp(st.stx_ctime),
    }
}

/// `wasi_unstable`'s `filestat`.
pub(crate) fn snapshot0_filestat(stat: Filestat) -> Snapshot0Filestat {
    Snapshot0Filestat {
        st_dev: stat.st_dev,
        st_ino: stat.st_ino,
        st_filetype: stat.st_filetype,
        st_nlink: u32::try_from(stat.st_nlink).unwrap_or(u32::MAX),
        st_size: stat.st_size,
        st_atim: stat.st_atim,
        st_mtim: stat.st_mtim,
        st_ctim: stat.st_ctim,
    }
}

/// The statx fields a v1 `filestat` needs.
pub(crate) const FILESTAT_MASK: StatxMask = StatxMask(
    StatxMask::STATX_TYPE.0
        | StatxMask::STATX_NLINK.0
        | StatxMask::STATX_INO.0
        | StatxMask::STATX_SIZE.0
        | StatxMask::STATX_ATIME.0
        | StatxMask::STATX_MTIME.0
        | StatxMask::STATX_CTIME.0,
);

/// Writes `stat` in the layout of the module the call came from.
pub(crate) async fn write_filestat<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    out: i32,
    stat: Filestat,
    snapshot0: bool,
) -> V1Result<()> {
    match snapshot0 {
        true => frame.write(out as u32, &snapshot0_filestat(stat)).await,
        false => frame.write(out as u32, &stat).await,
    }
}

/// Linux status flags for v1 `fdflags`. `RSYNC` is `O_SYNC`, as `O_RSYNC` is on Linux.
pub(crate) fn open_flags(flags: Fdflags) -> OpenFlags {
    [
        (Fdflags::APPEND, OpenFlags::O_APPEND),
        (Fdflags::DSYNC, OpenFlags::O_DSYNC),
        (Fdflags::NONBLOCK, OpenFlags::O_NONBLOCK),
        (Fdflags::RSYNC, OpenFlags::O_SYNC),
        (Fdflags::SYNC, OpenFlags::O_SYNC),
    ]
    .into_iter()
    .filter(|(v1, _)| flags.contains(*v1))
    .fold(OpenFlags(0), |all, (_, linux)| all | linux)
}

/// v1 `fdflags` for Linux status flags.
pub(crate) fn fdflags(flags: OpenFlags) -> Fdflags {
    [
        (OpenFlags::O_APPEND, Fdflags::APPEND),
        (OpenFlags::O_DSYNC, Fdflags::DSYNC),
        (OpenFlags::O_NONBLOCK, Fdflags::NONBLOCK),
        (OpenFlags::O_SYNC, Fdflags::SYNC),
    ]
    .into_iter()
    .filter(|(linux, _)| flags.contains(*linux))
    .fold(Fdflags::empty(), |all, (_, v1)| all | v1)
}

/// The rights v1 reports for an open file of type `filetype` with status flags `flags`:
/// what its access mode and type allow. wasix-libc's `fcntl(F_GETFL)` reads the access mode
/// back from `FD_READ`/`FD_WRITE`, and `isatty` is "a character device without `FD_SEEK` and
/// `FD_TELL`".
pub(crate) fn rights_base(filetype: Filetype, flags: OpenFlags, tty: bool) -> Rights {
    if flags.contains(OpenFlags::O_PATH) {
        return match filetype {
            Filetype::DIRECTORY => Rights::FD_FILESTAT_GET | Rights::PATH_ALL,
            _ => Rights::FD_FILESTAT_GET,
        };
    }
    let access = flags & OpenFlags::O_ACCMODE;
    let readable = access != OpenFlags::O_WRONLY;
    let writable = access == OpenFlags::O_WRONLY || access == OpenFlags::O_RDWR;
    let mut rights = Rights::FD_FDSTAT_SET_FLAGS | Rights::FD_FILESTAT_GET;
    if readable {
        rights |= Rights::FD_READ;
    }
    if writable {
        rights |= Rights::FD_WRITE | Rights::FD_DATASYNC | Rights::FD_SYNC;
    }
    rights |= match filetype {
        Filetype::REGULAR_FILE | Filetype::BLOCK_DEVICE => {
            let mut file = Rights::FD_SEEK
                | Rights::FD_TELL
                | Rights::FD_ADVISE
                | Rights::FD_FILESTAT_SET_TIMES
                | Rights::POLL_FD_READWRITE;
            if writable {
                file |= Rights::FD_ALLOCATE | Rights::FD_FILESTAT_SET_SIZE;
            }
            file
        }
        Filetype::DIRECTORY => {
            Rights::FD_READDIR
                | Rights::PATH_ALL
                | Rights::FD_FILESTAT_SET_TIMES
                | Rights::FD_SYNC
                | Rights::FD_DATASYNC
        }
        Filetype::CHARACTER_DEVICE if tty => {
            Rights::FD_FILESTAT_SET_TIMES | Rights::POLL_FD_READWRITE
        }
        Filetype::CHARACTER_DEVICE => {
            Rights::FD_SEEK
                | Rights::FD_TELL
                | Rights::FD_FILESTAT_SET_TIMES
                | Rights::POLL_FD_READWRITE
        }
        Filetype::SOCKET_STREAM
        | Filetype::SOCKET_DGRAM
        | Filetype::SOCKET_RAW
        | Filetype::SOCKET_SEQPACKET => Rights::SOCK_ALL | Rights::POLL_FD_READWRITE,
        Filetype::SYMBOLIC_LINK => Rights::empty(),
        _ => Rights::POLL_FD_READWRITE,
    };
    rights
}

/// The v1 `fdstat` of `fd`.
async fn fdstat<S: Syscalls + Control + 'static>(frame: &Frame<'_, S>, fd: Fd) -> V1Result<Fdstat> {
    let st = fstat(frame, fd, StatxMask::STATX_TYPE).await?;
    let flags = OpenFlags(
        frame
            .syscalls
            .fcntl64(&frame.caller(), fd, FcntlCmd::F_GETFL, 0)
            .await? as u32,
    );
    let filetype = fd_filetype(frame, fd, u32::from(st.stx_mode)).await?;
    let tty = filetype == Filetype::CHARACTER_DEVICE && is_tty(frame, fd).await?;
    Ok(Fdstat {
        fs_filetype: filetype,
        fs_flags: fdflags(flags),
        fs_rights_base: rights_base(filetype, flags, tty),
        fs_rights_inheriting: Rights::ALL,
    })
}

pub(crate) async fn fd_fdstat_get<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    out: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let stat = fdstat(frame, Fd(fd)).await?;
        frame.write(out as u32, &stat).await
    })
    .await
}

/// `F_SETFL`, keeping the status flags v1 cannot name. Linux ignores the sync flags here, as
/// it does for `fcntl`.
pub(crate) async fn fd_fdstat_set_flags<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    flags: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let caller = frame.caller();
        let old = frame
            .syscalls
            .fcntl64(&caller, Fd(fd), FcntlCmd::F_GETFL, 0)
            .await? as u32;
        let named = open_flags(Fdflags::ALL).0;
        let new = (old & !named) | open_flags(Fdflags(flags as u16)).0;
        frame
            .syscalls
            .fcntl64(&caller, Fd(fd), FcntlCmd::F_SETFL, new)
            .await?;
        Ok(())
    })
    .await
}

/// Succeeds for rights the descriptor has; nothing is enforced either way.
pub(crate) async fn fd_fdstat_set_rights<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    base: i64,
    _inheriting: i64,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let stat = fdstat(frame, Fd(fd)).await?;
        if !stat.fs_rights_base.contains(Rights(base as u64)) {
            return shim::fail(Errno::NOTCAPABLE);
        }
        Ok(())
    })
    .await
}

/// `fd_filestat_get` of `wasi_snapshot_preview1` and `wasix_32v1`.
pub(crate) async fn fd_filestat_get<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    out: i32,
) -> Result<i32, RuntimeError> {
    filestat_get(env, fd, out, false).await
}

/// `fd_filestat_get` of `wasi_unstable`.
pub(crate) async fn snapshot0_fd_filestat_get<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    out: i32,
) -> Result<i32, RuntimeError> {
    filestat_get(env, fd, out, true).await
}

async fn filestat_get<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    out: i32,
    snapshot0: bool,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let st = fstat(frame, Fd(fd), FILESTAT_MASK).await?;
        let filetype = fd_filetype(frame, Fd(fd), u32::from(st.stx_mode)).await?;
        write_filestat(frame, out, filestat(&st, filetype), snapshot0).await
    })
    .await
}

pub(crate) async fn fd_filestat_set_size<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    size: i64,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        frame
            .syscalls
            .ftruncate64(&frame.caller(), Fd(fd), size)
            .await?;
        Ok(())
    })
    .await
}

/// The `utimensat` times for v1's timestamps and `fstflags`.
pub(crate) fn utimes(atim: u64, mtim: u64, flags: Fstflags) -> V1Result<[KernelTimespec; 2]> {
    if (flags & !Fstflags::ALL).0 != 0 {
        return shim::fail(Errno::INVAL);
    }
    let time = |ns: u64, set: Fstflags, now: Fstflags| {
        if flags.contains(set) && flags.contains(now) {
            return shim::fail(Errno::INVAL);
        }
        let special = |nsec: UtimeSpecial| KernelTimespec {
            tv_sec: 0,
            tv_nsec: i64::from(nsec.0),
        };
        Ok(if flags.contains(now) {
            special(UtimeSpecial::UTIME_NOW)
        } else if flags.contains(set) {
            KernelTimespec {
                tv_sec: (ns / 1_000_000_000) as i64,
                tv_nsec: (ns % 1_000_000_000) as i64,
            }
        } else {
            special(UtimeSpecial::UTIME_OMIT)
        })
    };
    Ok([
        time(atim, Fstflags::SET_ATIM, Fstflags::SET_ATIM_NOW)?,
        time(mtim, Fstflags::SET_MTIM, Fstflags::SET_MTIM_NOW)?,
    ])
}

/// `utimensat(fd, NULL, times, 0)`, as musl's `futimens`.
pub(crate) async fn fd_filestat_set_times<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    atim: i64,
    mtim: i64,
    flags: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let times = utimes(atim as u64, mtim as u64, Fstflags(flags as u16))?;
        let times = frame.mem.alloc(&times)?;
        frame
            .syscalls
            .utimensat_time64(
                &frame.caller(),
                Fd(fd),
                GuestPtr::new(0),
                GuestPtr::new(times.addr()),
                AtFlags(0),
            )
            .await?;
        Ok(())
    })
    .await
}

/// The Linux `whence` for v1's.
fn whence(whence: u8, snapshot0: bool) -> Option<SeekWhence> {
    Some(match snapshot0 {
        false => match Whence(whence) {
            Whence::SET => SeekWhence::SEEK_SET,
            Whence::CUR => SeekWhence::SEEK_CUR,
            Whence::END => SeekWhence::SEEK_END,
            _ => return None,
        },
        true => match Snapshot0Whence(whence) {
            Snapshot0Whence::SET => SeekWhence::SEEK_SET,
            Snapshot0Whence::CUR => SeekWhence::SEEK_CUR,
            Snapshot0Whence::END => SeekWhence::SEEK_END,
            _ => return None,
        },
    })
}

/// `fd_seek` of `wasi_snapshot_preview1` and `wasix_32v1`.
pub(crate) async fn fd_seek<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    offset: i64,
    whence: i32,
    out: i32,
) -> Result<i32, RuntimeError> {
    seek(env, fd, offset, whence, out, false).await
}

/// `fd_seek` of `wasi_unstable`, whose `whence` numbers differ.
pub(crate) async fn snapshot0_fd_seek<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    offset: i64,
    whence: i32,
    out: i32,
) -> Result<i32, RuntimeError> {
    seek(env, fd, offset, whence, out, true).await
}

async fn seek<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    offset: i64,
    whence_v1: i32,
    out: i32,
    snapshot0: bool,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let Some(whence) = whence(whence_v1 as u8, snapshot0) else {
            return shim::fail(Errno::INVAL);
        };
        let pos = frame
            .syscalls
            .lseek(&frame.caller(), Fd(fd), offset, whence)
            .await?;
        frame.write(out as u32, &(pos as u64)).await
    })
    .await
}

pub(crate) async fn fd_tell<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    out: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let pos = frame
            .syscalls
            .lseek(&frame.caller(), Fd(fd), 0, SeekWhence::SEEK_CUR)
            .await?;
        frame.write(out as u32, &(pos as u64)).await
    })
    .await
}

/// `preadv`; the guest's iovec array goes to the kernel untranslated.
pub(crate) async fn fd_pread<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    iovs: i32,
    iovs_len: i32,
    offset: i64,
    nread: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let n = frame
            .syscalls
            .preadv(
                &frame.caller(),
                Fd(fd),
                ConstIovecArray::new(iovs as u32),
                iovs_len as u32,
                offset,
            )
            .await?;
        frame.write(nread as u32, &n).await
    })
    .await
}

/// `pwritev`. As on Linux, a file opened with `O_APPEND` is appended to whatever the offset.
pub(crate) async fn fd_pwrite<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    iovs: i32,
    iovs_len: i32,
    offset: i64,
    nwritten: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let n = frame
            .syscalls
            .pwritev(
                &frame.caller(),
                Fd(fd),
                ConstIovecArray::new(iovs as u32),
                iovs_len as u32,
                offset,
            )
            .await?;
        frame.write(nwritten as u32, &n).await
    })
    .await
}

pub(crate) async fn fd_sync<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        frame.syscalls.fsync(&frame.caller(), Fd(fd)).await?;
        Ok(())
    })
    .await
}

pub(crate) async fn fd_datasync<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        frame.syscalls.fdatasync(&frame.caller(), Fd(fd)).await?;
        Ok(())
    })
    .await
}

/// The Linux advice for v1's, whose numbering differs.
fn advice(advice: Advice) -> Option<FadviseAdvice> {
    Some(match advice {
        Advice::NORMAL => FadviseAdvice::POSIX_FADV_NORMAL,
        Advice::SEQUENTIAL => FadviseAdvice::POSIX_FADV_SEQUENTIAL,
        Advice::RANDOM => FadviseAdvice::POSIX_FADV_RANDOM,
        Advice::WILLNEED => FadviseAdvice::POSIX_FADV_WILLNEED,
        Advice::DONTNEED => FadviseAdvice::POSIX_FADV_DONTNEED,
        Advice::NOREUSE => FadviseAdvice::POSIX_FADV_NOREUSE,
        _ => return None,
    })
}

pub(crate) async fn fd_advise<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    offset: i64,
    len: i64,
    advice_v1: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let Some(advice) = advice(Advice(advice_v1 as u8)) else {
            return shim::fail(Errno::INVAL);
        };
        frame
            .syscalls
            .fadvise64_64(&frame.caller(), Fd(fd), offset, len, advice)
            .await?;
        Ok(())
    })
    .await
}

/// `fallocate(fd, 0, offset, len)`. A filesystem that cannot allocate gets what
/// `posix_fallocate` falls back to: the file grows to `offset + len` if it is shorter.
pub(crate) async fn fd_allocate<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    offset: i64,
    len: i64,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let ret = frame
            .syscalls
            .fallocate(
                &frame.caller(),
                Fd(fd),
                FallocFlags::FALLOC_FL_ALLOCATE_RANGE,
                offset,
                len,
            )
            .await;
        match answer(ret)? {
            Ok(_) => return Ok(()),
            Err(wasixv2_abi::Errno::EOPNOTSUPP) => {}
            Err(errno) => return Err(errno.into()),
        }
        let Some(end) = offset.checked_add(len) else {
            return Err(wasixv2_abi::Errno::EFBIG.into());
        };
        let st = fstat(frame, Fd(fd), StatxMask::STATX_SIZE).await?;
        if (end as u64) > st.stx_size {
            frame
                .syscalls
                .ftruncate64(&frame.caller(), Fd(fd), end)
                .await?;
        }
        Ok(())
    })
    .await
}

/// v1's `fd_renumber` is `dup2`: `to` becomes a copy of `from`, which stays open. wasix-libc
/// implements `dup2` and `dup3` with it.
pub(crate) async fn fd_renumber<S: Syscalls + Control + 'static>(
    env: Env<S>,
    from: i32,
    to: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let caller = frame.caller();
        if from == to {
            // `dup2` of a descriptor onto itself only checks that it is open.
            frame
                .syscalls
                .fcntl64(&caller, Fd(from), FcntlCmd::F_GETFD, 0)
                .await?;
            return Ok(());
        }
        frame
            .syscalls
            .dup3(&caller, Fd(from), Fd(to), OpenFlags(0))
            .await?;
        if is_preopen(frame, to).await? {
            frame.proc().await?.fs.replaced.lock().unwrap().insert(to);
        }
        Ok(())
    })
    .await
}

pub(crate) async fn fd_dup<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    out: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let new = frame.syscalls.dup(&frame.caller(), Fd(fd)).await?;
        frame.write(out as u32, &(new.0 as u32)).await
    })
    .await
}

/// `fcntl(F_DUPFD)`: the lowest free descriptor from `min_fd` up.
pub(crate) async fn fd_dup2<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    min_fd: i32,
    cloexec: i32,
    out: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let cmd = match cloexec as u8 {
            0 => FcntlCmd::F_DUPFD,
            _ => FcntlCmd::F_DUPFD_CLOEXEC,
        };
        let new = frame
            .syscalls
            .fcntl64(&frame.caller(), Fd(fd), cmd, min_fd as u32)
            .await?;
        frame.write(out as u32, &(new as u32)).await
    })
    .await
}

pub(crate) async fn fd_fdflags_get<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    out: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let flags = frame
            .syscalls
            .fcntl64(&frame.caller(), Fd(fd), FcntlCmd::F_GETFD, 0)
            .await?;
        let ext = match FdFlags(flags as u32).contains(FdFlags::FD_CLOEXEC) {
            true => Fdflagsext::CLOEXEC,
            false => Fdflagsext::empty(),
        };
        frame.write(out as u32, &ext).await
    })
    .await
}

pub(crate) async fn fd_fdflags_set<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    flags: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let cloexec = match Fdflagsext(flags as u16).contains(Fdflagsext::CLOEXEC) {
            true => FdFlags::FD_CLOEXEC,
            false => FdFlags(0),
        };
        frame
            .syscalls
            .fcntl64(&frame.caller(), Fd(fd), FcntlCmd::F_SETFD, cloexec.0)
            .await?;
        Ok(())
    })
    .await
}

/// `pipe2(fds, 0)`: the read end, then the write end.
pub(crate) async fn fd_pipe<S: Syscalls + Control + 'static>(
    env: Env<S>,
    read_out: i32,
    write_out: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let fds = frame.mem.alloc_mut::<[Fd; 2]>()?;
        frame
            .syscalls
            .pipe2(&frame.caller(), GuestPtrMut::new(fds.addr()), OpenFlags(0))
            .await?;
        let [read, write] = frame.mem.take(fds);
        frame.write(read_out as u32, &(read.0 as u32)).await?;
        frame.write(write_out as u32, &(write.0 as u32)).await
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_flags_round_trip() {
        for bit in 0..5 {
            let flags = Fdflags(1 << bit);
            let back = fdflags(open_flags(flags));
            // RSYNC has no Linux flag of its own: it reads back as SYNC (and DSYNC, which
            // O_SYNC contains).
            match flags {
                Fdflags::RSYNC | Fdflags::SYNC => {
                    assert_eq!(back, Fdflags::SYNC | Fdflags::DSYNC)
                }
                _ => assert_eq!(back, flags),
            }
        }
        assert_eq!(
            open_flags(Fdflags::APPEND | Fdflags::NONBLOCK),
            OpenFlags::O_APPEND | OpenFlags::O_NONBLOCK
        );
    }

    #[test]
    fn whence_numbering() {
        assert_eq!(whence(0, false), Some(SeekWhence::SEEK_SET));
        assert_eq!(whence(2, false), Some(SeekWhence::SEEK_END));
        assert_eq!(whence(0, true), Some(SeekWhence::SEEK_CUR));
        assert_eq!(whence(2, true), Some(SeekWhence::SEEK_SET));
        assert_eq!(whence(3, false), None);
    }

    #[test]
    fn advice_numbering() {
        assert_eq!(
            advice(Advice::SEQUENTIAL),
            Some(FadviseAdvice::POSIX_FADV_SEQUENTIAL)
        );
        assert_eq!(
            advice(Advice::RANDOM),
            Some(FadviseAdvice::POSIX_FADV_RANDOM)
        );
        assert_eq!(advice(Advice(6)), None);
    }

    #[test]
    fn rights_follow_access_mode_and_type() {
        let file = rights_base(Filetype::REGULAR_FILE, OpenFlags::O_RDONLY, false);
        assert!(file.contains(Rights::FD_READ | Rights::FD_SEEK | Rights::FD_TELL));
        assert!(!file.intersects(Rights::FD_WRITE | Rights::FD_ALLOCATE));

        let file = rights_base(Filetype::REGULAR_FILE, OpenFlags::O_WRONLY, false);
        assert!(file.contains(Rights::FD_WRITE | Rights::FD_FILESTAT_SET_SIZE));
        assert!(!file.contains(Rights::FD_READ));

        let dir = rights_base(Filetype::DIRECTORY, OpenFlags::O_RDONLY, false);
        assert!(dir.contains(Rights::FD_READDIR | Rights::PATH_OPEN | Rights::PATH_ALL));
        assert!(!dir.contains(Rights::FD_WRITE));

        // isatty: a character device without seek and tell.
        let tty = rights_base(Filetype::CHARACTER_DEVICE, OpenFlags::O_RDWR, true);
        assert!(!tty.intersects(Rights::FD_SEEK | Rights::FD_TELL));
        let null = rights_base(Filetype::CHARACTER_DEVICE, OpenFlags::O_RDWR, false);
        assert!(null.contains(Rights::FD_SEEK | Rights::FD_TELL));

        let sock = rights_base(Filetype::SOCKET_STREAM, OpenFlags::O_RDWR, false);
        assert!(sock.contains(Rights::SOCK_ALL | Rights::FD_READ | Rights::FD_WRITE));

        let path = rights_base(Filetype::REGULAR_FILE, OpenFlags::O_PATH, false);
        assert_eq!(path, Rights::FD_FILESTAT_GET);
        assert!(Rights::ALL.contains(Rights::PATH_ALL | Rights::SOCK_ALL));
    }

    #[test]
    fn file_types() {
        assert_eq!(filetype_of_mode(0o100644), Filetype::REGULAR_FILE);
        assert_eq!(filetype_of_mode(0o040755), Filetype::DIRECTORY);
        assert_eq!(filetype_of_mode(0o120777), Filetype::SYMBOLIC_LINK);
        assert_eq!(filetype_of_mode(0o020666), Filetype::CHARACTER_DEVICE);
        assert_eq!(filetype_of_mode(0o060660), Filetype::BLOCK_DEVICE);
        assert_eq!(filetype_of_mode(0o140777), Filetype::SOCKET_STREAM);
        assert_eq!(filetype_of_mode(0o010600), Filetype::UNKNOWN);
        assert_eq!(
            socket_filetype(SocketType::SOCK_DGRAM),
            Filetype::SOCKET_DGRAM
        );
    }

    #[test]
    fn filestat_encoding() {
        let st = Statx {
            stx_nlink: 3,
            stx_ino: 42,
            stx_size: 1000,
            stx_dev_major: 8,
            stx_dev_minor: 1,
            stx_atime: StatxTimestamp {
                tv_sec: 2,
                tv_nsec: 5,
                reserved: 0,
            },
            stx_mtime: StatxTimestamp {
                tv_sec: -1,
                tv_nsec: 0,
                reserved: 0,
            },
            ..Statx::default()
        };
        let stat = filestat(&st, Filetype::REGULAR_FILE);
        assert_eq!(stat.st_dev, 0x801);
        assert_eq!(stat.st_ino, 42);
        assert_eq!(stat.st_nlink, 3);
        assert_eq!(stat.st_size, 1000);
        assert_eq!(stat.st_atim, 2_000_000_005);
        // Before the epoch has no v1 representation.
        assert_eq!(stat.st_mtim, 0);
        let old = snapshot0_filestat(stat);
        assert_eq!((old.st_nlink, old.st_size), (3, 1000));
    }

    #[test]
    fn utimes_from_fstflags() {
        let [atim, mtim] = utimes(
            1_500_000_000,
            0,
            Fstflags::SET_ATIM | Fstflags::SET_MTIM_NOW,
        )
        .unwrap();
        assert_eq!((atim.tv_sec, atim.tv_nsec), (1, 500_000_000));
        assert_eq!(mtim.tv_nsec, i64::from(UtimeSpecial::UTIME_NOW.0));
        let [atim, _] = utimes(0, 0, Fstflags::empty()).unwrap();
        assert_eq!(atim.tv_nsec, i64::from(UtimeSpecial::UTIME_OMIT.0));
        assert!(utimes(0, 0, Fstflags::SET_ATIM | Fstflags::SET_ATIM_NOW).is_err());
        assert!(utimes(0, 0, Fstflags(0x10)).is_err());
    }
}
