//! Plain data shared by the VFS and its drivers.

use wasixv2_abi::{Errno, FileMode};

pub type FsResult<T> = Result<T, Errno>;

/// The type of a filesystem object, fixed when it is created.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileType {
    Regular,
    Directory,
    Symlink,
    Fifo,
    Socket,
    CharDevice,
    BlockDevice,
}

impl FileType {
    /// The `S_IFMT` bits.
    pub fn mode_bits(self) -> u32 {
        let mode = match self {
            Self::Regular => FileMode::S_IFREG,
            Self::Directory => FileMode::S_IFDIR,
            Self::Symlink => FileMode::S_IFLNK,
            Self::Fifo => FileMode::S_IFIFO,
            Self::Socket => FileMode::S_IFSOCK,
            Self::CharDevice => FileMode::S_IFCHR,
            Self::BlockDevice => FileMode::S_IFBLK,
        };
        mode.0
    }

    /// The type named by the `S_IFMT` bits of `mode`.
    pub fn from_mode(mode: u32) -> Option<Self> {
        Some(match FileMode(mode & FileMode::S_IFMT.0) {
            FileMode::S_IFREG => Self::Regular,
            FileMode::S_IFDIR => Self::Directory,
            FileMode::S_IFLNK => Self::Symlink,
            FileMode::S_IFIFO => Self::Fifo,
            FileMode::S_IFSOCK => Self::Socket,
            FileMode::S_IFCHR => Self::CharDevice,
            FileMode::S_IFBLK => Self::BlockDevice,
            _ => return None,
        })
    }

    /// The `d_type` of a `getdents64` record (`DT_*`, which is `S_IFMT >> 12`).
    pub fn dirent_type(self) -> u8 {
        (self.mode_bits() >> 12) as u8
    }
}

/// Permission bits including set-uid, set-gid and sticky (`S_IALLUGO`).
pub const MODE_PERM: u32 = 0o7777;
pub const S_ISUID: u32 = 0o4000;
pub const S_ISGID: u32 = 0o2000;
pub const S_ISVTX: u32 = 0o1000;
pub const S_IXGRP: u32 = 0o010;
pub const S_IXUGO: u32 = 0o111;

/// A point in time as seconds and nanoseconds since the Unix epoch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp {
    pub sec: i64,
    /// Always below 1_000_000_000.
    pub nsec: u32,
}

impl Timestamp {
    pub const fn new(sec: i64, nsec: u32) -> Self {
        Self { sec, nsec }
    }
}

/// The time source drivers stamp metadata with.
pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

/// A device number. Linux encodes it as 12 bits of major and 20 of minor in a `dev_t`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Device {
    pub major: u32,
    pub minor: u32,
}

impl Device {
    /// Decodes the `dev` argument of `mknod` as Linux's `new_decode_dev` does.
    pub fn from_dev_t(dev: u32) -> Self {
        Self {
            major: (dev & 0xfff00) >> 8,
            minor: (dev & 0xff) | ((dev >> 12) & 0xfff00),
        }
    }
}

/// Metadata of a filesystem object, as `statx` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attr {
    pub ino: u64,
    pub file_type: FileType,
    /// Permission bits, `MODE_PERM` at most.
    pub mode: u32,
    pub nlink: u32,
    pub uid: u32,
    pub gid: u32,
    /// For device nodes, the device they refer to.
    pub rdev: Device,
    pub size: u64,
    /// Space used, in 512-byte units.
    pub blocks: u64,
    /// Preferred I/O size.
    pub blksize: u32,
    pub atime: Timestamp,
    pub mtime: Timestamp,
    pub ctime: Timestamp,
    /// Creation time, if the filesystem records one.
    pub btime: Option<Timestamp>,
    /// A device of the driver's own that the object lives on, when it is not the filesystem's
    /// (a host directory can span host mounts). The VFS gives each such device its own
    /// anonymous `st_dev`, so the value is only compared, never shown. `None` is the
    /// filesystem's device.
    pub dev: Option<Device>,
}

impl Attr {
    /// `st_mode`: type and permission bits.
    pub fn st_mode(&self) -> u32 {
        self.file_type.mode_bits() | self.mode
    }
}

/// A metadata change. `None` fields are left alone. The VFS decides every value, including the
/// `ctime` that accompanies most changes; drivers store what they are given.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SetAttr {
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    /// Truncates or extends with a hole, as `truncate` does.
    pub size: Option<u64>,
    pub atime: Option<TimeSet>,
    pub mtime: Option<TimeSet>,
    pub ctime: Option<TimeSet>,
}

impl SetAttr {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// A timestamp to store: a given value, or the driver's clock (which for a host directory is
/// the host's, so it matches what the host's own updates write).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeSet {
    Now,
    At(Timestamp),
}

/// What `create` makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewKind<'a> {
    Regular,
    Directory,
    Symlink(&'a [u8]),
    Fifo,
    Socket,
    CharDevice(Device),
    BlockDevice(Device),
}

impl NewKind<'_> {
    pub fn file_type(&self) -> FileType {
        match self {
            Self::Regular => FileType::Regular,
            Self::Directory => FileType::Directory,
            Self::Symlink(_) => FileType::Symlink,
            Self::Fifo => FileType::Fifo,
            Self::Socket => FileType::Socket,
            Self::CharDevice(_) => FileType::CharDevice,
            Self::BlockDevice(_) => FileType::BlockDevice,
        }
    }
}

/// A new object: its kind and initial ownership and permission bits, already decided by the VFS
/// (umask applied, set-gid inheritance resolved).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewNode<'a> {
    pub kind: NewKind<'a>,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
}

/// How `rename` treats an existing target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenameMode {
    /// Replace the target atomically, as `rename(2)`.
    Replace,
    /// Fail with `EEXIST` if the target exists (`RENAME_NOREPLACE`).
    NoReplace,
    /// Swap source and target, which must both exist (`RENAME_EXCHANGE`).
    Exchange,
    /// Replace and leave a whiteout at the source (`RENAME_WHITEOUT`).
    Whiteout,
}

/// One directory entry produced by `readdir`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirEntry<'a> {
    pub name: &'a [u8],
    pub ino: u64,
    pub file_type: FileType,
    /// The cookie that resumes reading after this entry.
    pub next: u64,
}

/// Receives `readdir` entries; returns `false` when it is full and wants no more.
pub trait DirSink {
    fn push(&mut self, entry: DirEntry<'_>) -> bool;
}

/// How an object is opened for data access.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpenOptions {
    pub read: bool,
    pub write: bool,
    /// Do not update the access time on reads (`O_NOATIME`).
    pub noatime: bool,
    /// Writes reach stable storage before returning (`O_SYNC`/`O_DSYNC`).
    pub sync: bool,
}

/// `fallocate` modes the VFS passes through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllocMode {
    /// Allocate, extending the size if the range ends past it.
    Allocate,
    /// Allocate without changing the size (`FALLOC_FL_KEEP_SIZE`).
    AllocateKeepSize,
    /// Deallocate the range; it reads as zeroes and the size is unchanged.
    PunchHole,
    /// Zero the range (`FALLOC_FL_ZERO_RANGE`), optionally keeping the size.
    ZeroRange { keep_size: bool },
}

/// `SEEK_DATA` or `SEEK_HOLE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeekHole {
    Data,
    Hole,
}

/// Where an extended attribute set may create or replace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XattrSet {
    Any,
    /// `XATTR_CREATE`: fail with `EEXIST` if it exists.
    Create,
    /// `XATTR_REPLACE`: fail with `ENODATA` if it does not.
    Replace,
}

/// Filesystem-wide numbers for `statfs`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StatFs {
    pub block_size: u32,
    pub blocks: u64,
    pub blocks_free: u64,
    pub blocks_avail: u64,
    pub files: u64,
    pub files_free: u64,
    pub name_max: u32,
    pub fragment_size: u32,
}

/// What the VFS needs to know about a filesystem up front.
#[derive(Clone, Debug)]
pub struct FsInfo {
    /// The filesystem type's name, as `/proc/self/mounts` shows it (`tmpfs`, `proc`).
    pub name: &'static str,
    /// `f_type` of `statfs` (e.g. `TMPFS_MAGIC`).
    pub magic: u32,
    /// Longest name a directory entry can have.
    pub name_max: usize,
    /// Nothing on it can change, whatever the mount says.
    pub read_only: bool,
    /// The device all its objects live on; `None` asks the VFS for an anonymous one, as Linux's
    /// `get_anon_bdev` does for tmpfs.
    pub dev: Option<Device>,
    /// The driver records creation times.
    pub has_btime: bool,
}

/// `TMPFS_MAGIC`.
pub const TMPFS_MAGIC: u32 = 0x0102_1994;
