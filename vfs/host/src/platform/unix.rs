use std::ffi::CString;
use std::hash::{Hash, Hasher};
use std::io;
use std::mem;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::path::Path;

use vfs_core::provider::FsProviderCapabilities;
use vfs_core::{BackendInodeId, VfsFileType, VfsTimespec};

use super::{DirEntryInfo, Stat};

#[derive(Debug)]
pub struct DirHandle {
    fd: OwnedFd,
}

impl Clone for DirHandle {
    fn clone(&self) -> Self {
        let fd = unsafe { libc::dup(self.fd.as_raw_fd()) };
        if fd < 0 {
            panic!("dup dirfd failed: {}", io::Error::last_os_error());
        }
        unsafe {
            Self {
                fd: OwnedFd::from_raw_fd(fd),
            }
        }
    }
}

impl DirHandle {
    pub fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

pub fn provider_capabilities() -> FsProviderCapabilities {
    FsProviderCapabilities::SYMLINK
        | FsProviderCapabilities::HARDLINK
        | FsProviderCapabilities::UNIX_PERMISSIONS
        | FsProviderCapabilities::UTIMENS
        | FsProviderCapabilities::SEEK
        | FsProviderCapabilities::CASE_SENSITIVE
}

pub fn open_root_dir(path: &Path) -> io::Result<DirHandle> {
    let cstr = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC;
    let fd = unsafe { libc::open(cstr.as_ptr(), flags) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(DirHandle {
        fd: unsafe { OwnedFd::from_raw_fd(fd) },
    })
}

pub fn open_dir_at(parent: &DirHandle, name: &vfs_core::VfsName) -> io::Result<DirHandle> {
    let cstr = CString::new(name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains NUL"))?;
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW;
    let fd = unsafe { libc::openat(parent.as_raw_fd(), cstr.as_ptr(), flags) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(DirHandle {
        fd: unsafe { OwnedFd::from_raw_fd(fd) },
    })
}

pub fn stat_root(dir: &DirHandle) -> io::Result<Stat> {
    let mut st = unsafe { mem::zeroed::<libc::stat>() };
    let res = unsafe { libc::fstat(dir.as_raw_fd(), &mut st) };
    if res < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(stat_from_libc(&st))
}

pub fn stat_dir(dir: &DirHandle) -> io::Result<Stat> {
    stat_root(dir)
}

pub fn stat_at(parent: &DirHandle, name: &[u8], nofollow: bool) -> io::Result<Stat> {
    let cstr =
        CString::new(name).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name NUL"))?;
    let mut st = unsafe { mem::zeroed::<libc::stat>() };
    let flags = if nofollow {
        libc::AT_SYMLINK_NOFOLLOW
    } else {
        0
    };
    let res = unsafe { libc::fstatat(parent.as_raw_fd(), cstr.as_ptr(), &mut st, flags) };
    if res < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(stat_from_libc(&st))
}

pub fn stat_file(file: &std::fs::File) -> io::Result<Stat> {
    let mut st = unsafe { mem::zeroed::<libc::stat>() };
    let res = unsafe { libc::fstat(file.as_raw_fd(), &mut st) };
    if res < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(stat_from_libc(&st))
}

pub fn open_file_at(
    parent: &DirHandle,
    name: &[u8],
    flags: vfs_core::flags::OpenFlags,
    mode: Option<u32>,
) -> io::Result<std::fs::File> {
    let cstr =
        CString::new(name).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name NUL"))?;
    let mut oflags = libc::O_CLOEXEC;
    if flags.contains(vfs_core::flags::OpenFlags::NOFOLLOW) {
        oflags |= libc::O_NOFOLLOW;
    }
    if flags.contains(vfs_core::flags::OpenFlags::DIRECTORY) {
        oflags |= libc::O_DIRECTORY;
    }
    if flags.contains(vfs_core::flags::OpenFlags::TRUNC) {
        oflags |= libc::O_TRUNC;
    }
    if flags.contains(vfs_core::flags::OpenFlags::CREATE) {
        oflags |= libc::O_CREAT;
    }
    if flags.contains(vfs_core::flags::OpenFlags::EXCL) {
        oflags |= libc::O_EXCL;
    }
    if flags.contains(vfs_core::flags::OpenFlags::APPEND) {
        oflags |= libc::O_APPEND;
    }
    if flags.contains(vfs_core::flags::OpenFlags::SYNC) {
        oflags |= libc::O_SYNC;
    }
    #[cfg(target_os = "linux")]
    if flags.contains(vfs_core::flags::OpenFlags::DSYNC) {
        oflags |= libc::O_DSYNC;
    }

    let access = if flags.contains(vfs_core::flags::OpenFlags::READ)
        && flags.contains(vfs_core::flags::OpenFlags::WRITE)
    {
        libc::O_RDWR
    } else if flags.contains(vfs_core::flags::OpenFlags::WRITE) {
        libc::O_WRONLY
    } else {
        libc::O_RDONLY
    };
    oflags |= access;
    let mode = mode.unwrap_or(0o666) as libc::mode_t;
    let fd = unsafe { libc::openat(parent.as_raw_fd(), cstr.as_ptr(), oflags, mode) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { std::fs::File::from_raw_fd(fd) })
}

pub fn mkdir_at(parent: &DirHandle, name: &[u8], mode: Option<u32>) -> io::Result<()> {
    let cstr =
        CString::new(name).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name NUL"))?;
    let mode = mode.unwrap_or(0o777) as libc::mode_t;
    let res = unsafe { libc::mkdirat(parent.as_raw_fd(), cstr.as_ptr(), mode) };
    if res < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn unlink_at(parent: &DirHandle, name: &[u8]) -> io::Result<()> {
    let cstr =
        CString::new(name).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name NUL"))?;
    let res = unsafe { libc::unlinkat(parent.as_raw_fd(), cstr.as_ptr(), 0) };
    if res < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn rmdir_at(parent: &DirHandle, name: &[u8]) -> io::Result<()> {
    let cstr =
        CString::new(name).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name NUL"))?;
    let res = unsafe { libc::unlinkat(parent.as_raw_fd(), cstr.as_ptr(), libc::AT_REMOVEDIR) };
    if res < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn rename_at(
    old_parent: &DirHandle,
    old_name: &[u8],
    new_parent: &DirHandle,
    new_name: &[u8],
) -> io::Result<()> {
    let old_cstr = CString::new(old_name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name NUL"))?;
    let new_cstr = CString::new(new_name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name NUL"))?;
    let res = unsafe {
        libc::renameat(
            old_parent.as_raw_fd(),
            old_cstr.as_ptr(),
            new_parent.as_raw_fd(),
            new_cstr.as_ptr(),
        )
    };
    if res < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn link_at(
    existing_parent: &DirHandle,
    existing_name: &[u8],
    new_parent: &DirHandle,
    new_name: &[u8],
) -> io::Result<()> {
    let old_cstr = CString::new(existing_name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name NUL"))?;
    let new_cstr = CString::new(new_name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name NUL"))?;
    let res = unsafe {
        libc::linkat(
            existing_parent.as_raw_fd(),
            old_cstr.as_ptr(),
            new_parent.as_raw_fd(),
            new_cstr.as_ptr(),
            0,
        )
    };
    if res < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn symlink_at(parent: &DirHandle, new_name: &[u8], target: &[u8]) -> io::Result<()> {
    let new_cstr = CString::new(new_name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name NUL"))?;
    let target_cstr = CString::new(target)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "target NUL"))?;
    let res =
        unsafe { libc::symlinkat(target_cstr.as_ptr(), parent.as_raw_fd(), new_cstr.as_ptr()) };
    if res < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn readlink_at(parent: &DirHandle, name: &[u8]) -> io::Result<Vec<u8>> {
    let cstr =
        CString::new(name).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name NUL"))?;
    let mut buf = vec![0u8; 1024];
    loop {
        let res = unsafe {
            libc::readlinkat(
                parent.as_raw_fd(),
                cstr.as_ptr(),
                buf.as_mut_ptr() as *mut _,
                buf.len(),
            )
        };
        if res < 0 {
            return Err(io::Error::last_os_error());
        }
        let len = res as usize;
        if len < buf.len() {
            buf.truncate(len);
            return Ok(buf);
        }
        buf.resize(buf.len() * 2, 0);
    }
}

pub fn chmod_at(parent: &DirHandle, name: &[u8], mode: u32) -> io::Result<()> {
    let cstr =
        CString::new(name).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name NUL"))?;
    let res = unsafe { libc::fchmodat(parent.as_raw_fd(), cstr.as_ptr(), mode as _, 0) };
    if res < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn chown_at(
    parent: &DirHandle,
    name: &[u8],
    uid: Option<u32>,
    gid: Option<u32>,
) -> io::Result<()> {
    let cstr =
        CString::new(name).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name NUL"))?;
    let uid = uid.map(|v| v as libc::uid_t).unwrap_or(!0);
    let gid = gid.map(|v| v as libc::gid_t).unwrap_or(!0);
    let res = unsafe { libc::fchownat(parent.as_raw_fd(), cstr.as_ptr(), uid, gid, 0) };
    if res < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn utimens_at(
    parent: &DirHandle,
    name: &[u8],
    atime: Option<VfsTimespec>,
    mtime: Option<VfsTimespec>,
) -> io::Result<()> {
    let cstr =
        CString::new(name).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name NUL"))?;
    let times = make_utimenspec(atime, mtime)?;
    let res = unsafe { libc::utimensat(parent.as_raw_fd(), cstr.as_ptr(), times.as_ptr(), 0) };
    if res < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn read_dir(dir: &DirHandle) -> io::Result<Vec<DirEntryInfo>> {
    let dup_fd = unsafe { libc::dup(dir.as_raw_fd()) };
    if dup_fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let dirp = unsafe { libc::fdopendir(dup_fd) };
    if dirp.is_null() {
        unsafe { libc::close(dup_fd) };
        return Err(io::Error::last_os_error());
    }

    let mut entries = Vec::new();
    loop {
        set_errno(0);
        let ent = unsafe { libc::readdir(dirp) };
        if ent.is_null() {
            let err = errno();
            if err == 0 {
                break;
            }
            unsafe { libc::closedir(dirp) };
            return Err(io::Error::from_raw_os_error(err));
        }
        let name = unsafe { std::ffi::CStr::from_ptr((*ent).d_name.as_ptr()) }
            .to_bytes()
            .to_vec();
        if name == b"." || name == b".." {
            continue;
        }
        let stat = stat_at(dir, &name, true)?;
        entries.push(DirEntryInfo {
            name,
            inode: stat.inode,
            file_type: stat.file_type,
        });
    }
    unsafe { libc::closedir(dirp) };
    Ok(entries)
}

fn stat_from_libc(st: &libc::stat) -> Stat {
    let file_type = if (st.st_mode & libc::S_IFMT) == libc::S_IFDIR {
        VfsFileType::Directory
    } else if (st.st_mode & libc::S_IFMT) == libc::S_IFLNK {
        VfsFileType::Symlink
    } else {
        VfsFileType::RegularFile
    };
    let inode = make_backend_inode(st.st_dev as u64, st.st_ino as u64);
    let (atime, mtime, ctime) = stat_times(st);
    Stat {
        inode,
        file_type,
        mode: (st.st_mode & 0o7777) as u32,
        uid: st.st_uid,
        gid: st.st_gid,
        nlink: st.st_nlink as u64,
        size: st.st_size as u64,
        atime,
        mtime,
        ctime,
        rdev_major: 0,
        rdev_minor: 0,
        dir_handle: None,
    }
}

fn make_backend_inode(dev: u64, ino: u64) -> BackendInodeId {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    dev.hash(&mut hasher);
    ino.hash(&mut hasher);
    let mut raw = hasher.finish();
    if raw == 0 {
        raw = 1;
    }
    BackendInodeId::new(raw).expect("non-zero inode")
}

#[cfg(target_os = "macos")]
fn stat_times(st: &libc::stat) -> (VfsTimespec, VfsTimespec, VfsTimespec) {
    let atime = VfsTimespec {
        secs: st.st_atimespec.tv_sec,
        nanos: st.st_atimespec.tv_nsec as u32,
    };
    let mtime = VfsTimespec {
        secs: st.st_mtimespec.tv_sec,
        nanos: st.st_mtimespec.tv_nsec as u32,
    };
    let ctime = VfsTimespec {
        secs: st.st_ctimespec.tv_sec,
        nanos: st.st_ctimespec.tv_nsec as u32,
    };
    (atime, mtime, ctime)
}

#[cfg(not(target_os = "macos"))]
fn stat_times(st: &libc::stat) -> (VfsTimespec, VfsTimespec, VfsTimespec) {
    let atime = VfsTimespec {
        secs: st.st_atime as i64,
        nanos: st.st_atime_nsec as u32,
    };
    let mtime = VfsTimespec {
        secs: st.st_mtime as i64,
        nanos: st.st_mtime_nsec as u32,
    };
    let ctime = VfsTimespec {
        secs: st.st_ctime as i64,
        nanos: st.st_ctime_nsec as u32,
    };
    (atime, mtime, ctime)
}

fn make_utimenspec(
    atime: Option<VfsTimespec>,
    mtime: Option<VfsTimespec>,
) -> io::Result<[libc::timespec; 2]> {
    let mut times = [
        libc::timespec {
            tv_sec: 0,
            tv_nsec: libc::UTIME_OMIT,
        },
        libc::timespec {
            tv_sec: 0,
            tv_nsec: libc::UTIME_OMIT,
        },
    ];
    if let Some(atime) = atime {
        times[0].tv_sec = atime.secs;
        times[0].tv_nsec = atime.nanos as _;
    }
    if let Some(mtime) = mtime {
        times[1].tv_sec = mtime.secs;
        times[1].tv_nsec = mtime.nanos as _;
    }
    Ok(times)
}

#[cfg(target_os = "linux")]
fn errno() -> i32 {
    unsafe { *libc::__errno_location() }
}

#[cfg(target_os = "macos")]
fn errno() -> i32 {
    unsafe { *libc::__error() }
}

#[cfg(target_os = "linux")]
fn set_errno(val: i32) {
    unsafe {
        *libc::__errno_location() = val;
    }
}

#[cfg(target_os = "macos")]
fn set_errno(val: i32) {
    unsafe {
        *libc::__error() = val;
    }
}
