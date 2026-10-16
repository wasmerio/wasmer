//! The Linux implementation: `O_PATH` descriptors and `*at` calls, run on the host executor's
//! blocking pool.

use std::{
    any::Any,
    ffi::CString,
    io,
    mem::MaybeUninit,
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
    path::Path,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use wasixv2_abi::Errno;
use wasixv2_host::Host;
use wasixv2_vfs::{
    AllocMode, Attr, Device, DirEntry, DirSink, FileType, Filesystem, FsInfo, FsResult, Handle,
    MAX_DIR_COOKIE, NAME_MAX, NewKind, NewNode, Node, OpenOptions, RenameMode, SeekHole, SetAttr,
    StatFs, TimeSet, Timestamp, XattrSet,
};

use crate::Options;

const XATTR_SIZE_MAX: usize = 65536;

fn last_errno() -> Errno {
    Errno(
        io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EIO),
    )
}

/// Host errno values are Linux's generic ones, the same numbers the guest ABI uses.
fn cvt(ret: libc::c_int) -> FsResult<libc::c_int> {
    if ret < 0 { Err(last_errno()) } else { Ok(ret) }
}

fn cvt_size(ret: isize) -> FsResult<usize> {
    if ret < 0 {
        Err(last_errno())
    } else {
        Ok(ret as usize)
    }
}

fn cstring(bytes: &[u8]) -> FsResult<CString> {
    CString::new(bytes).map_err(|_| Errno::EINVAL)
}

/// The path that reopens descriptor `fd` (a magic link to its inode).
fn proc_path(fd: RawFd) -> CString {
    CString::new(format!("/proc/self/fd/{fd}")).expect("no NUL in a number")
}

fn open_at(
    dir: RawFd,
    name: &CString,
    flags: libc::c_int,
    mode: libc::c_uint,
) -> FsResult<OwnedFd> {
    // SAFETY: `name` is NUL-terminated; the returned descriptor is owned by nobody else.
    let fd = cvt(unsafe { libc::openat(dir, name.as_ptr(), flags | libc::O_CLOEXEC, mode) })?;
    // SAFETY: `fd` was just opened.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn stat_fd(fd: RawFd) -> FsResult<libc::stat> {
    let mut st = MaybeUninit::<libc::stat>::uninit();
    let empty = c"";
    // SAFETY: `st` is written by the call on success.
    cvt(unsafe {
        libc::fstatat(
            fd,
            empty.as_ptr(),
            st.as_mut_ptr(),
            libc::AT_EMPTY_PATH | libc::AT_SYMLINK_NOFOLLOW,
        )
    })?;
    // SAFETY: initialized by a successful `fstatat`.
    Ok(unsafe { st.assume_init() })
}

fn file_type(mode: libc::mode_t) -> FileType {
    FileType::from_mode(mode).unwrap_or(FileType::Regular)
}

struct Shared {
    host: Arc<dyn Host>,
    options: Options,
    host_uid: u32,
    host_gid: u32,
    /// The host device of the mapped directory.
    root_dev: libc::dev_t,
    /// Other host devices seen below it (host mounts inside the directory), by index - 1.
    other_devs: Mutex<Vec<libc::dev_t>>,
}

impl Shared {
    async fn blocking<T, F>(&self, work: F) -> T
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        self.host.executor().run_blocking(work).await
    }

    fn writable(&self) -> FsResult<()> {
        if self.options.read_only {
            return Err(Errno::EROFS);
        }
        Ok(())
    }

    fn to_guest(&self, uid: u32, gid: u32) -> (u32, u32) {
        let uid = if uid == self.host_uid {
            self.options.owner.0
        } else {
            uid
        };
        let gid = if gid == self.host_gid {
            self.options.owner.1
        } else {
            gid
        };
        (uid, gid)
    }

    fn to_host(&self, uid: Option<u32>, gid: Option<u32>) -> (u32, u32) {
        let map = |id: Option<u32>, guest: u32, host: u32| match id {
            None => u32::MAX,
            Some(id) if id == guest => host,
            Some(id) => id,
        };
        (
            map(uid, self.options.owner.0, self.host_uid),
            map(gid, self.options.owner.1, self.host_gid),
        )
    }

    /// The inode number the VFS sees. Inode numbers are only unique per host device, and the
    /// VFS needs them unique per filesystem, so objects on another host device than the mapped
    /// directory get the device's index folded into the high bits, as overlayfs's `xino` does.
    fn ino(&self, dev: libc::dev_t, ino: u64) -> u64 {
        if dev == self.root_dev {
            return ino;
        }
        let mut devs = self.other_devs.lock().unwrap();
        let index = match devs.iter().position(|&d| d == dev) {
            Some(index) => index,
            None => {
                devs.push(dev);
                devs.len() - 1
            }
        };
        ino ^ ((index as u64 + 1) << 48)
    }

    fn attr(&self, st: &libc::stat) -> Attr {
        let (uid, gid) = self.to_guest(st.st_uid, st.st_gid);
        let device = |dev: libc::dev_t| Device {
            major: libc::major(dev),
            minor: libc::minor(dev),
        };
        Attr {
            ino: self.ino(st.st_dev, st.st_ino),
            file_type: file_type(st.st_mode),
            mode: st.st_mode & 0o7777,
            nlink: st.st_nlink as u32,
            uid,
            gid,
            rdev: device(st.st_rdev),
            size: st.st_size as u64,
            blocks: st.st_blocks as u64,
            blksize: st.st_blksize as u32,
            atime: Timestamp::new(st.st_atime, st.st_atime_nsec as u32),
            mtime: Timestamp::new(st.st_mtime, st.st_mtime_nsec as u32),
            ctime: Timestamp::new(st.st_ctime, st.st_ctime_nsec as u32),
            btime: None,
            // Objects on other host devices keep a device of their own, which the VFS maps to
            // one of its anonymous devices.
            dev: (st.st_dev != self.root_dev).then(|| device(st.st_dev)),
        }
    }
}

/// A host directory as a filesystem.
pub struct HostFs {
    shared: Arc<Shared>,
    root: Arc<HostNode>,
}

impl HostFs {
    pub fn new(root: &Path, host: Arc<dyn Host>, options: Options) -> io::Result<Self> {
        let path = CString::new(root.as_os_str().as_encoded_bytes())?;
        let fd = open_at(libc::AT_FDCWD, &path, libc::O_PATH | libc::O_DIRECTORY, 0)
            .map_err(|errno| io::Error::from_raw_os_error(errno.0))?;
        let st = stat_fd(fd.as_raw_fd()).map_err(|errno| io::Error::from_raw_os_error(errno.0))?;
        // SAFETY: plain getters.
        let (host_uid, host_gid) = unsafe { (libc::geteuid(), libc::getegid()) };
        let shared = Arc::new(Shared {
            host,
            options,
            host_uid,
            host_gid,
            root_dev: st.st_dev,
            other_devs: Mutex::default(),
        });
        let root = HostNode::new(&shared, fd, &st);
        Ok(Self { shared, root })
    }
}

#[async_trait(?Send)]
impl Filesystem for HostFs {
    fn info(&self) -> FsInfo {
        FsInfo {
            name: "9p",
            magic: 0x0102_1997, // V9FS_MAGIC: a passthrough filesystem
            name_max: NAME_MAX,
            read_only: self.shared.options.read_only,
            dev: None,
            has_btime: false,
        }
    }

    fn root(&self) -> Arc<dyn Node> {
        self.root.clone()
    }

    async fn statfs(&self) -> FsResult<StatFs> {
        let fd = self.root.fd.clone();
        self.shared
            .blocking(move || {
                let mut st = MaybeUninit::<libc::statfs>::uninit();
                // SAFETY: `st` is written on success.
                cvt(unsafe { libc::fstatfs(fd.as_raw_fd(), st.as_mut_ptr()) })?;
                // SAFETY: initialized by a successful `fstatfs`.
                let st = unsafe { st.assume_init() };
                Ok(StatFs {
                    block_size: st.f_bsize as u32,
                    blocks: st.f_blocks,
                    blocks_free: st.f_bfree,
                    blocks_avail: st.f_bavail,
                    files: st.f_files,
                    files_free: st.f_ffree,
                    name_max: st.f_namelen as u32,
                    fragment_size: st.f_frsize as u32,
                })
            })
            .await
    }

    async fn sync(&self) -> FsResult<()> {
        let fd = self.root.fd.clone();
        self.shared
            .blocking(move || {
                let dir = open_at(fd.as_raw_fd(), &CString::from(c"."), libc::O_RDONLY, 0)?;
                // SAFETY: plain syscall on an owned descriptor.
                cvt(unsafe { libc::syncfs(dir.as_raw_fd()) }).map(drop)
            })
            .await
    }
}

/// A host object, held by an `O_PATH` descriptor.
pub struct HostNode {
    fd: Arc<OwnedFd>,
    ino: u64,
    dev: libc::dev_t,
    file_type: FileType,
    shared: Arc<Shared>,
}

impl HostNode {
    fn new(shared: &Arc<Shared>, fd: OwnedFd, st: &libc::stat) -> Arc<Self> {
        Arc::new(Self {
            fd: Arc::new(fd),
            ino: shared.ino(st.st_dev, st.st_ino),
            dev: st.st_dev,
            file_type: file_type(st.st_mode),
            shared: shared.clone(),
        })
    }

    fn downcast(node: &Arc<dyn Node>) -> FsResult<&HostNode> {
        node.as_any().downcast_ref::<HostNode>().ok_or(Errno::EXDEV)
    }

    /// Opens `name` in this directory as a node.
    async fn child(&self, name: &[u8]) -> FsResult<Arc<HostNode>> {
        let name = cstring(name)?;
        let dir = self.fd.clone();
        let (fd, st) = self
            .shared
            .blocking(move || {
                let fd = open_at(dir.as_raw_fd(), &name, libc::O_PATH | libc::O_NOFOLLOW, 0)?;
                let st = stat_fd(fd.as_raw_fd())?;
                Ok::<_, Errno>((fd, st))
            })
            .await?;
        Ok(HostNode::new(&self.shared, fd, &st))
    }

    /// Runs `op` with this directory's descriptor and `name`.
    async fn at<T: Send + 'static>(
        &self,
        name: &[u8],
        op: impl FnOnce(RawFd, &CString) -> FsResult<T> + Send + 'static,
    ) -> FsResult<T> {
        let name = cstring(name)?;
        let dir = self.fd.clone();
        self.shared
            .blocking(move || op(dir.as_raw_fd(), &name))
            .await
    }

    /// Runs `op` with the `/proc/self/fd` path of this object.
    async fn via_proc<T: Send + 'static>(
        &self,
        op: impl FnOnce(&CString) -> FsResult<T> + Send + 'static,
    ) -> FsResult<T> {
        let fd = self.fd.clone();
        self.shared
            .blocking(move || op(&proc_path(fd.as_raw_fd())))
            .await
    }
}

fn timespec(time: Option<TimeSet>) -> libc::timespec {
    match time {
        None => libc::timespec {
            tv_sec: 0,
            tv_nsec: libc::UTIME_OMIT,
        },
        Some(TimeSet::Now) => libc::timespec {
            tv_sec: 0,
            tv_nsec: libc::UTIME_NOW,
        },
        Some(TimeSet::At(t)) => libc::timespec {
            tv_sec: t.sec,
            tv_nsec: i64::from(t.nsec),
        },
    }
}

#[async_trait(?Send)]
impl Node for HostNode {
    fn ino(&self) -> u64 {
        self.ino
    }

    fn file_type(&self) -> FileType {
        self.file_type
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn getattr(&self) -> FsResult<Attr> {
        let fd = self.fd.clone();
        let st = self
            .shared
            .blocking(move || stat_fd(fd.as_raw_fd()))
            .await?;
        Ok(self.shared.attr(&st))
    }

    async fn setattr(&self, attr: &SetAttr) -> FsResult<Attr> {
        self.shared.writable()?;
        let attr = attr.clone();
        let (uid, gid) = self.shared.to_host(attr.uid, attr.gid);
        let fd = self.fd.clone();
        let is_symlink = self.file_type == FileType::Symlink;
        self.shared
            .blocking(move || {
                let raw = fd.as_raw_fd();
                let proc = proc_path(raw);
                if attr.uid.is_some() || attr.gid.is_some() {
                    // SAFETY: plain syscall; "" with AT_EMPTY_PATH names `raw` itself.
                    cvt(unsafe {
                        libc::fchownat(raw, c"".as_ptr(), uid, gid, libc::AT_EMPTY_PATH)
                    })?;
                }
                if let Some(mode) = attr.mode {
                    // SAFETY: plain syscall on a NUL-terminated path.
                    cvt(unsafe { libc::chmod(proc.as_ptr(), mode) })?;
                }
                if let Some(size) = attr.size {
                    let size = i64::try_from(size).map_err(|_| Errno::EFBIG)?;
                    // SAFETY: plain syscall on a NUL-terminated path.
                    cvt(unsafe { libc::truncate(proc.as_ptr(), size) })?;
                }
                // The host stamps ctime itself; it cannot be set.
                if (attr.atime.is_some() || attr.mtime.is_some()) && !is_symlink {
                    let times = [timespec(attr.atime), timespec(attr.mtime)];
                    // SAFETY: `times` holds two timespecs.
                    cvt(unsafe {
                        libc::utimensat(libc::AT_FDCWD, proc.as_ptr(), times.as_ptr(), 0)
                    })?;
                }
                stat_fd(raw)
            })
            .await
            .map(|st| self.shared.attr(&st))
    }

    async fn lookup(&self, name: &[u8]) -> FsResult<Arc<dyn Node>> {
        Ok(self.child(name).await?)
    }

    async fn create(&self, name: &[u8], new: &NewNode<'_>) -> FsResult<Arc<dyn Node>> {
        self.shared.writable()?;
        let mode = new.mode;
        let kind = match new.kind {
            NewKind::Regular => 0,
            NewKind::Directory => 1,
            NewKind::Symlink(_) => 2,
            NewKind::Fifo => 3,
            NewKind::Socket => 4,
            NewKind::CharDevice(_) | NewKind::BlockDevice(_) => return Err(Errno::EPERM),
        };
        let target = match new.kind {
            NewKind::Symlink(target) => Some(cstring(target)?),
            _ => None,
        };
        let (uid, gid) = self.shared.to_host(Some(new.uid), Some(new.gid));
        self.at(name, move |dir, name| {
            // SAFETY: plain syscalls on NUL-terminated names relative to `dir`.
            unsafe {
                match kind {
                    0 => drop(open_at(
                        dir,
                        name,
                        libc::O_CREAT | libc::O_EXCL | libc::O_WRONLY | libc::O_NOFOLLOW,
                        mode,
                    )?),
                    1 => cvt(libc::mkdirat(dir, name.as_ptr(), mode)).map(drop)?,
                    2 => cvt(libc::symlinkat(
                        target.as_ref().expect("symlink target").as_ptr(),
                        dir,
                        name.as_ptr(),
                    ))
                    .map(drop)?,
                    3 => {
                        cvt(libc::mknodat(dir, name.as_ptr(), libc::S_IFIFO | mode, 0)).map(drop)?
                    }
                    _ => cvt(libc::mknodat(dir, name.as_ptr(), libc::S_IFSOCK | mode, 0))
                        .map(drop)?,
                }
            }
            let fd = open_at(dir, name, libc::O_PATH | libc::O_NOFOLLOW, 0)?;
            if kind != 2 {
                // The host umask applied at creation; set the mode the VFS decided.
                let proc = proc_path(fd.as_raw_fd());
                // SAFETY: plain syscall.
                cvt(unsafe { libc::chmod(proc.as_ptr(), mode) })?;
            }
            // Best effort: only a privileged host user can give files away.
            // SAFETY: plain syscall.
            unsafe {
                libc::fchownat(fd.as_raw_fd(), c"".as_ptr(), uid, gid, libc::AT_EMPTY_PATH);
            }
            let st = stat_fd(fd.as_raw_fd())?;
            Ok((fd, st))
        })
        .await
        .map(|(fd, st)| HostNode::new(&self.shared, fd, &st) as Arc<dyn Node>)
    }

    async fn create_unnamed(&self, new: &NewNode<'_>) -> FsResult<Arc<dyn Node>> {
        self.shared.writable()?;
        let mode = new.mode;
        let (fd, st) = self
            .at(b".", move |dir, dot| {
                let fd = open_at(dir, dot, libc::O_TMPFILE | libc::O_RDWR, mode)?;
                let st = stat_fd(fd.as_raw_fd())?;
                Ok((fd, st))
            })
            .await?;
        Ok(HostNode::new(&self.shared, fd, &st))
    }

    async fn link(&self, name: &[u8], target: &Arc<dyn Node>) -> FsResult<()> {
        self.shared.writable()?;
        let target = HostNode::downcast(target)?.fd.clone();
        self.at(name, move |dir, name| {
            let source = proc_path(target.as_raw_fd());
            // SAFETY: plain syscall; following the magic link names the held inode.
            cvt(unsafe {
                libc::linkat(
                    libc::AT_FDCWD,
                    source.as_ptr(),
                    dir,
                    name.as_ptr(),
                    libc::AT_SYMLINK_FOLLOW,
                )
            })
            .map(drop)
        })
        .await
    }

    async fn unlink(&self, name: &[u8]) -> FsResult<()> {
        self.shared.writable()?;
        self.at(name, |dir, name| {
            // SAFETY: plain syscall.
            cvt(unsafe { libc::unlinkat(dir, name.as_ptr(), 0) }).map(drop)
        })
        .await
    }

    async fn rmdir(&self, name: &[u8]) -> FsResult<()> {
        self.shared.writable()?;
        self.at(name, |dir, name| {
            // SAFETY: plain syscall.
            cvt(unsafe { libc::unlinkat(dir, name.as_ptr(), libc::AT_REMOVEDIR) }).map(drop)
        })
        .await
    }

    async fn rename(
        &self,
        name: &[u8],
        new_dir: &Arc<dyn Node>,
        new_name: &[u8],
        mode: RenameMode,
    ) -> FsResult<()> {
        self.shared.writable()?;
        let flags = match mode {
            RenameMode::Replace => 0,
            RenameMode::NoReplace => libc::RENAME_NOREPLACE,
            RenameMode::Exchange => libc::RENAME_EXCHANGE,
            RenameMode::Whiteout => return Err(Errno::EINVAL),
        };
        let new_dir = HostNode::downcast(new_dir)?.fd.clone();
        let new_name = cstring(new_name)?;
        self.at(name, move |dir, name| {
            // SAFETY: plain syscall on NUL-terminated names.
            cvt(unsafe {
                libc::renameat2(
                    dir,
                    name.as_ptr(),
                    new_dir.as_raw_fd(),
                    new_name.as_ptr(),
                    flags,
                )
            })
            .map(drop)
        })
        .await
    }

    async fn readlink(&self) -> FsResult<Vec<u8>> {
        let fd = self.fd.clone();
        self.shared
            .blocking(move || {
                let mut buf = vec![0u8; libc::PATH_MAX as usize];
                // SAFETY: `buf` is writable for its length; "" names the O_PATH symlink itself.
                let n = cvt_size(unsafe {
                    libc::readlinkat(
                        fd.as_raw_fd(),
                        c"".as_ptr(),
                        buf.as_mut_ptr().cast(),
                        buf.len(),
                    )
                })?;
                buf.truncate(n);
                Ok(buf)
            })
            .await
    }

    async fn open(&self, options: OpenOptions) -> FsResult<Box<dyn Handle>> {
        let fd = self.fd.clone();
        let is_dir = self.file_type == FileType::Directory;
        let read_only = self.shared.options.read_only;
        let file = self
            .shared
            .blocking(move || {
                if is_dir {
                    return open_at(
                        fd.as_raw_fd(),
                        &CString::from(c"."),
                        libc::O_RDONLY | libc::O_DIRECTORY,
                        0,
                    );
                }
                if options.write && read_only {
                    return Err(Errno::EROFS);
                }
                let mut flags = match (options.read, options.write) {
                    (_, false) => libc::O_RDONLY,
                    (false, true) => libc::O_WRONLY,
                    (true, true) => libc::O_RDWR,
                };
                if options.sync {
                    flags |= libc::O_DSYNC;
                }
                let proc = proc_path(fd.as_raw_fd());
                match open_at(
                    libc::AT_FDCWD,
                    &proc,
                    flags | if options.noatime { libc::O_NOATIME } else { 0 },
                    0,
                ) {
                    // The host may refuse O_NOATIME where the guest owns the file.
                    Err(Errno::EPERM) if options.noatime => {
                        open_at(libc::AT_FDCWD, &proc, flags, 0)
                    }
                    result => result,
                }
            })
            .await?;
        Ok(Box::new(HostHandle {
            file: Arc::new(file),
            shared: self.shared.clone(),
            appender: Mutex::new(None),
            dir: Mutex::new(DirState::default()),
            dev: self.dev,
        }))
    }

    async fn getxattr(&self, name: &[u8]) -> FsResult<Vec<u8>> {
        let name = cstring(name)?;
        self.via_proc(move |path| {
            let mut buf = vec![0u8; XATTR_SIZE_MAX];
            // SAFETY: `buf` is writable for its length.
            let n = cvt_size(unsafe {
                libc::getxattr(
                    path.as_ptr(),
                    name.as_ptr(),
                    buf.as_mut_ptr().cast(),
                    buf.len(),
                )
            })?;
            buf.truncate(n);
            Ok(buf)
        })
        .await
    }

    async fn setxattr(&self, name: &[u8], value: &[u8], how: XattrSet) -> FsResult<()> {
        self.shared.writable()?;
        let name = cstring(name)?;
        let value = value.to_vec();
        let flags = match how {
            XattrSet::Any => 0,
            XattrSet::Create => libc::XATTR_CREATE,
            XattrSet::Replace => libc::XATTR_REPLACE,
        };
        self.via_proc(move |path| {
            // SAFETY: `value` is readable for its length.
            cvt(unsafe {
                libc::setxattr(
                    path.as_ptr(),
                    name.as_ptr(),
                    value.as_ptr().cast(),
                    value.len(),
                    flags,
                )
            })
            .map(drop)
        })
        .await
    }

    async fn listxattr(&self) -> FsResult<Vec<Vec<u8>>> {
        self.via_proc(|path| {
            let mut buf = vec![0u8; XATTR_SIZE_MAX];
            // SAFETY: `buf` is writable for its length.
            let n = cvt_size(unsafe {
                libc::listxattr(path.as_ptr(), buf.as_mut_ptr().cast(), buf.len())
            })?;
            Ok(buf[..n]
                .split(|&b| b == 0)
                .filter(|name| !name.is_empty())
                .map(<[u8]>::to_vec)
                .collect())
        })
        .await
    }

    async fn removexattr(&self, name: &[u8]) -> FsResult<()> {
        self.shared.writable()?;
        let name = cstring(name)?;
        self.via_proc(move |path| {
            // SAFETY: plain syscall.
            cvt(unsafe { libc::removexattr(path.as_ptr(), name.as_ptr()) }).map(drop)
        })
        .await
    }
}

/// Where a directory handle's cookies point: the host offset after each entry it returned.
/// Cookie `n` resumes after the `n`th entry; host offsets themselves can be 64-bit hashes,
/// which do not fit a 32-bit `telldir`.
#[derive(Default)]
struct DirState {
    offsets: Vec<i64>,
}

/// An open host file or directory.
struct HostHandle {
    file: Arc<OwnedFd>,
    shared: Arc<Shared>,
    /// An `O_APPEND` descriptor for atomic appends, opened on first use.
    appender: Mutex<Option<Arc<OwnedFd>>>,
    dir: Mutex<DirState>,
    /// The host device the object is on, for the inode numbers of directory entries.
    dev: libc::dev_t,
}

impl HostHandle {
    async fn run<T: Send + 'static>(
        &self,
        op: impl FnOnce(RawFd) -> FsResult<T> + Send + 'static,
    ) -> FsResult<T> {
        let fd = self.file.clone();
        self.shared.blocking(move || op(fd.as_raw_fd())).await
    }

    async fn appender(&self) -> FsResult<Arc<OwnedFd>> {
        if let Some(fd) = &*self.appender.lock().unwrap() {
            return Ok(fd.clone());
        }
        let fd = self
            .run(|fd| {
                open_at(
                    libc::AT_FDCWD,
                    &proc_path(fd),
                    libc::O_WRONLY | libc::O_APPEND,
                    0,
                )
            })
            .await?;
        let fd = Arc::new(fd);
        *self.appender.lock().unwrap() = Some(fd.clone());
        Ok(fd)
    }
}

#[async_trait(?Send)]
impl Handle for HostHandle {
    async fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        let len = buf.len();
        let data = self
            .run(move |fd| {
                let mut data = vec![0u8; len];
                // SAFETY: `data` is writable for its length.
                let n = cvt_size(unsafe {
                    libc::pread(fd, data.as_mut_ptr().cast(), len, offset as i64)
                })?;
                data.truncate(n);
                Ok(data)
            })
            .await?;
        buf[..data.len()].copy_from_slice(&data);
        Ok(data.len())
    }

    async fn write_at(&self, data: &[u8], offset: u64) -> FsResult<usize> {
        let data = data.to_vec();
        self.run(move |fd| {
            // SAFETY: `data` is readable for its length.
            cvt_size(unsafe { libc::pwrite(fd, data.as_ptr().cast(), data.len(), offset as i64) })
        })
        .await
    }

    async fn append(&self, data: &[u8]) -> FsResult<(u64, usize)> {
        let fd = self.appender().await?;
        let data = data.to_vec();
        self.shared
            .blocking(move || {
                let fd = fd.as_raw_fd();
                // SAFETY: `data` is readable for its length.
                let n = cvt_size(unsafe { libc::write(fd, data.as_ptr().cast(), data.len()) })?;
                // SAFETY: plain syscall.
                let end = unsafe { libc::lseek(fd, 0, libc::SEEK_CUR) };
                if end < 0 {
                    return Err(last_errno());
                }
                Ok((end as u64 - n as u64, n))
            })
            .await
    }

    async fn sync(&self, data_only: bool) -> FsResult<()> {
        self.run(move |fd| {
            // SAFETY: plain syscalls.
            cvt(unsafe {
                if data_only {
                    libc::fdatasync(fd)
                } else {
                    libc::fsync(fd)
                }
            })
            .map(drop)
        })
        .await
    }

    async fn allocate(&self, mode: AllocMode, offset: u64, len: u64) -> FsResult<()> {
        let mode = match mode {
            AllocMode::Allocate => 0,
            AllocMode::AllocateKeepSize => libc::FALLOC_FL_KEEP_SIZE,
            AllocMode::PunchHole => libc::FALLOC_FL_PUNCH_HOLE | libc::FALLOC_FL_KEEP_SIZE,
            AllocMode::ZeroRange { keep_size: true } => {
                libc::FALLOC_FL_ZERO_RANGE | libc::FALLOC_FL_KEEP_SIZE
            }
            AllocMode::ZeroRange { keep_size: false } => libc::FALLOC_FL_ZERO_RANGE,
        };
        self.run(move |fd| {
            // SAFETY: plain syscall.
            cvt(unsafe { libc::fallocate(fd, mode, offset as i64, len as i64) }).map(drop)
        })
        .await
    }

    async fn seek_hole(&self, offset: u64, _size: u64, which: SeekHole) -> FsResult<u64> {
        let whence = match which {
            SeekHole::Data => libc::SEEK_DATA,
            SeekHole::Hole => libc::SEEK_HOLE,
        };
        self.run(move |fd| {
            // SAFETY: plain syscall; this descriptor's position is not used for I/O.
            let at = unsafe { libc::lseek(fd, offset as i64, whence) };
            if at < 0 {
                Err(last_errno())
            } else {
                Ok(at as u64)
            }
        })
        .await
    }

    async fn readdir(&self, cookie: u64, sink: &mut dyn DirSink) -> FsResult<()> {
        let start = match cookie {
            0 => 0,
            n => {
                let dir = self.dir.lock().unwrap();
                *dir.offsets.get(n as usize - 1).ok_or(Errno::EINVAL)?
            }
        };
        let entries = self
            .run(move |fd| {
                // SAFETY: plain syscall.
                if unsafe { libc::lseek(fd, start, libc::SEEK_SET) } < 0 {
                    return Err(last_errno());
                }
                read_host_dir(fd)
            })
            .await?;
        let mut index = cookie as usize;
        for entry in entries {
            if entry.name == b"." || entry.name == b".." {
                continue;
            }
            {
                let mut dir = self.dir.lock().unwrap();
                dir.offsets.truncate(index);
                dir.offsets.push(entry.off);
            }
            let next = (index as u64 + 1).min(MAX_DIR_COOKIE);
            let accepted = sink.push(DirEntry {
                name: &entry.name,
                ino: self.shared.ino(self.dev, entry.ino),
                file_type: entry.file_type,
                next,
            });
            if !accepted {
                break;
            }
            index += 1;
        }
        Ok(())
    }
}

struct HostDirent {
    name: Vec<u8>,
    ino: u64,
    off: i64,
    file_type: FileType,
}

/// Reads the rest of a host directory with `getdents64`.
fn read_host_dir(fd: RawFd) -> FsResult<Vec<HostDirent>> {
    let mut out = Vec::new();
    let mut buf = vec![0u8; 32 * 1024];
    loop {
        // SAFETY: `buf` is writable for its length.
        let n = unsafe { libc::syscall(libc::SYS_getdents64, fd, buf.as_mut_ptr(), buf.len()) };
        if n < 0 {
            return Err(last_errno());
        }
        if n == 0 {
            return Ok(out);
        }
        let mut pos = 0;
        while pos < n as usize {
            let record = &buf[pos..];
            let ino = u64::from_ne_bytes(record[0..8].try_into().unwrap());
            let off = i64::from_ne_bytes(record[8..16].try_into().unwrap());
            let reclen = u16::from_ne_bytes(record[16..18].try_into().unwrap()) as usize;
            let d_type = record[18];
            let name = &record[19..reclen];
            let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
            let file_type = match FileType::from_mode(u32::from(d_type) << 12) {
                Some(t) => t,
                None => {
                    // DT_UNKNOWN: ask the inode.
                    let cname = cstring(name)?;
                    let child = open_at(fd, &cname, libc::O_PATH | libc::O_NOFOLLOW, 0)?;
                    file_type(stat_fd(child.as_raw_fd())?.st_mode)
                }
            };
            out.push(HostDirent {
                name: name.to_vec(),
                ino,
                off,
                file_type,
            });
            pos += reclen;
        }
    }
}
