use crate::*;
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use std::fmt;
use wasmer_types::ValueType;

pub type __wasi_device_t = u64;

pub type __wasi_fd_t = u32;
pub const __WASI_STDIN_FILENO: u32 = 0;
pub const __WASI_STDOUT_FILENO: u32 = 1;
pub const __WASI_STDERR_FILENO: u32 = 2;

pub type __wasi_fdflags_t = u16;
pub const __WASI_FDFLAG_APPEND: u16 = 1 << 0;
pub const __WASI_FDFLAG_DSYNC: u16 = 1 << 1;
pub const __WASI_FDFLAG_NONBLOCK: u16 = 1 << 2;
pub const __WASI_FDFLAG_RSYNC: u16 = 1 << 3;
pub const __WASI_FDFLAG_SYNC: u16 = 1 << 4;

pub type __wasi_preopentype_t = u8;
pub const __WASI_PREOPENTYPE_DIR: u8 = 0;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct __wasi_prestat_u_dir_t {
    pub pr_name_len: u32,
}

unsafe impl ValueType for __wasi_prestat_u_dir_t {}

#[derive(Copy, Clone)]
#[repr(C)]
pub union __wasi_prestat_u {
    dir: __wasi_prestat_u_dir_t,
}

impl fmt::Debug for __wasi_prestat_u {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "__wasi_prestat_u")
    }
}

unsafe impl ValueType for __wasi_prestat_u {}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct __wasi_prestat_t {
    pub pr_type: __wasi_preopentype_t,
    pub u: __wasi_prestat_u,
}

#[derive(Copy, Clone)]
pub enum PrestatEnum {
    Dir { pr_name_len: u32 },
}

impl PrestatEnum {
    pub fn untagged(self) -> __wasi_prestat_u {
        match self {
            PrestatEnum::Dir { pr_name_len } => __wasi_prestat_u {
                dir: __wasi_prestat_u_dir_t { pr_name_len },
            },
        }
    }
}

impl __wasi_prestat_t {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn tagged(&self) -> Option<PrestatEnum> {
        match self.pr_type {
            __WASI_PREOPENTYPE_DIR => Some(PrestatEnum::Dir {
                pr_name_len: unsafe { self.u.dir.pr_name_len },
            }),
            _ => None,
        }
    }
}

unsafe impl ValueType for __wasi_prestat_t {}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct __wasi_fdstat_t {
    pub fs_filetype: __wasi_filetype_t,
    pub fs_flags: __wasi_fdflags_t,
    pub fs_rights_base: __wasi_rights_t,
    pub fs_rights_inheriting: __wasi_rights_t,
}

unsafe impl ValueType for __wasi_fdstat_t {}

pub type __wasi_filedelta_t = i64;

pub type __wasi_filesize_t = u64;

#[derive(Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[repr(C)]
pub struct __wasi_filestat_t {
    pub st_dev: __wasi_device_t,
    pub st_ino: __wasi_inode_t,
    pub st_filetype: __wasi_filetype_t,
    pub st_nlink: __wasi_linkcount_t,
    pub st_size: __wasi_filesize_t,
    pub st_atim: __wasi_timestamp_t,
    pub st_mtim: __wasi_timestamp_t,
    pub st_ctim: __wasi_timestamp_t,
}

impl Default for __wasi_filestat_t {
    fn default() -> Self {
        __wasi_filestat_t {
            st_dev: Default::default(),
            st_ino: Default::default(),
            st_filetype: __WASI_FILETYPE_UNKNOWN,
            st_nlink: 1,
            st_size: Default::default(),
            st_atim: Default::default(),
            st_mtim: Default::default(),
            st_ctim: Default::default(),
        }
    }
}

impl fmt::Debug for __wasi_filestat_t {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let convert_ts_into_time_string = |ts| {
            let tspec = ::time::OffsetDateTime::from_unix_timestamp_nanos(ts);
            format!("{} ({})", tspec.format("%a, %d %b %Y %T %z"), ts)
        };
        f.debug_struct("__wasi_filestat_t")
            .field("st_dev", &self.st_dev)
            .field("st_ino", &self.st_ino)
            .field(
                "st_filetype",
                &format!(
                    "{} ({})",
                    wasi_filetype_to_name(self.st_filetype),
                    self.st_filetype,
                ),
            )
            .field("st_nlink", &self.st_nlink)
            .field("st_size", &self.st_size)
            .field(
                "st_atim",
                &convert_ts_into_time_string(self.st_atim as i128),
            )
            .field(
                "st_mtim",
                &convert_ts_into_time_string(self.st_mtim as i128),
            )
            .field(
                "st_ctim",
                &convert_ts_into_time_string(self.st_ctim as i128),
            )
            .finish()
    }
}

unsafe impl ValueType for __wasi_filestat_t {}

pub fn wasi_filetype_to_name(ft: __wasi_filetype_t) -> &'static str {
    match ft {
        __WASI_FILETYPE_UNKNOWN => "Unknown",
        __WASI_FILETYPE_BLOCK_DEVICE => "Block device",
        __WASI_FILETYPE_CHARACTER_DEVICE => "Character device",
        __WASI_FILETYPE_DIRECTORY => "Directory",
        __WASI_FILETYPE_REGULAR_FILE => "Regular file",
        __WASI_FILETYPE_SOCKET_DGRAM => "Socket dgram",
        __WASI_FILETYPE_SOCKET_STREAM => "Socket stream",
        __WASI_FILETYPE_SYMBOLIC_LINK => "Symbolic link",
        _ => "Invalid",
    }
}

pub type __wasi_filetype_t = u8;
pub const __WASI_FILETYPE_UNKNOWN: u8 = 0;
pub const __WASI_FILETYPE_BLOCK_DEVICE: u8 = 1;
pub const __WASI_FILETYPE_CHARACTER_DEVICE: u8 = 2;
pub const __WASI_FILETYPE_DIRECTORY: u8 = 3;
pub const __WASI_FILETYPE_REGULAR_FILE: u8 = 4;
pub const __WASI_FILETYPE_SOCKET_DGRAM: u8 = 5;
pub const __WASI_FILETYPE_SOCKET_STREAM: u8 = 6;
pub const __WASI_FILETYPE_SYMBOLIC_LINK: u8 = 7;

pub type __wasi_fstflags_t = u16;
pub const __WASI_FILESTAT_SET_ATIM: u16 = 1 << 0;
pub const __WASI_FILESTAT_SET_ATIM_NOW: u16 = 1 << 1;
pub const __WASI_FILESTAT_SET_MTIM: u16 = 1 << 2;
pub const __WASI_FILESTAT_SET_MTIM_NOW: u16 = 1 << 3;

pub type __wasi_inode_t = u64;

pub type __wasi_linkcount_t = u64;

pub type __wasi_lookupflags_t = u32;
pub const __WASI_LOOKUP_SYMLINK_FOLLOW: u32 = 1 << 0;

pub type __wasi_oflags_t = u16;
pub const __WASI_O_CREAT: u16 = 1 << 0;
pub const __WASI_O_DIRECTORY: u16 = 1 << 1;
pub const __WASI_O_EXCL: u16 = 1 << 2;
pub const __WASI_O_TRUNC: u16 = 1 << 3;

pub type __wasi_rights_t = u64;
pub const __WASI_RIGHT_FD_DATASYNC: u64 = 1 << 0;
pub const __WASI_RIGHT_FD_READ: u64 = 1 << 1;
pub const __WASI_RIGHT_FD_SEEK: u64 = 1 << 2;
pub const __WASI_RIGHT_FD_FDSTAT_SET_FLAGS: u64 = 1 << 3;
pub const __WASI_RIGHT_FD_SYNC: u64 = 1 << 4;
pub const __WASI_RIGHT_FD_TELL: u64 = 1 << 5;
pub const __WASI_RIGHT_FD_WRITE: u64 = 1 << 6;
pub const __WASI_RIGHT_FD_ADVISE: u64 = 1 << 7;
pub const __WASI_RIGHT_FD_ALLOCATE: u64 = 1 << 8;
pub const __WASI_RIGHT_PATH_CREATE_DIRECTORY: u64 = 1 << 9;
pub const __WASI_RIGHT_PATH_CREATE_FILE: u64 = 1 << 10;
pub const __WASI_RIGHT_PATH_LINK_SOURCE: u64 = 1 << 11;
pub const __WASI_RIGHT_PATH_LINK_TARGET: u64 = 1 << 12;
pub const __WASI_RIGHT_PATH_OPEN: u64 = 1 << 13;
pub const __WASI_RIGHT_FD_READDIR: u64 = 1 << 14;
pub const __WASI_RIGHT_PATH_READLINK: u64 = 1 << 15;
pub const __WASI_RIGHT_PATH_RENAME_SOURCE: u64 = 1 << 16;
pub const __WASI_RIGHT_PATH_RENAME_TARGET: u64 = 1 << 17;
pub const __WASI_RIGHT_PATH_FILESTAT_GET: u64 = 1 << 18;
pub const __WASI_RIGHT_PATH_FILESTAT_SET_SIZE: u64 = 1 << 19;
pub const __WASI_RIGHT_PATH_FILESTAT_SET_TIMES: u64 = 1 << 20;
pub const __WASI_RIGHT_FD_FILESTAT_GET: u64 = 1 << 21;
pub const __WASI_RIGHT_FD_FILESTAT_SET_SIZE: u64 = 1 << 22;
pub const __WASI_RIGHT_FD_FILESTAT_SET_TIMES: u64 = 1 << 23;
pub const __WASI_RIGHT_PATH_SYMLINK: u64 = 1 << 24;
pub const __WASI_RIGHT_PATH_REMOVE_DIRECTORY: u64 = 1 << 25;
pub const __WASI_RIGHT_PATH_UNLINK_FILE: u64 = 1 << 26;
pub const __WASI_RIGHT_POLL_FD_READWRITE: u64 = 1 << 27;
pub const __WASI_RIGHT_SOCK_SHUTDOWN: u64 = 1 << 28;

/// function for debugging rights issues
#[allow(dead_code)]
pub fn print_right_set(rights: __wasi_rights_t) {
    // BTreeSet for consistent order
    let mut right_set = std::collections::BTreeSet::new();
    for i in 0..28 {
        let cur_right = rights & (1 << i);
        if cur_right != 0 {
            right_set.insert(right_to_string(cur_right).unwrap_or("INVALID RIGHT"));
        }
    }
    println!("{:#?}", right_set);
}

/// expects a single right, returns None if out of bounds or > 1 bit set
pub fn right_to_string(right: __wasi_rights_t) -> Option<&'static str> {
    Some(match right {
        __WASI_RIGHT_FD_DATASYNC => "__WASI_RIGHT_FD_DATASYNC",
        __WASI_RIGHT_FD_READ => "__WASI_RIGHT_FD_READ",
        __WASI_RIGHT_FD_SEEK => "__WASI_RIGHT_FD_SEEK",
        __WASI_RIGHT_FD_FDSTAT_SET_FLAGS => "__WASI_RIGHT_FD_FDSTAT_SET_FLAGS",
        __WASI_RIGHT_FD_SYNC => "__WASI_RIGHT_FD_SYNC",
        __WASI_RIGHT_FD_TELL => "__WASI_RIGHT_FD_TELL",
        __WASI_RIGHT_FD_WRITE => "__WASI_RIGHT_FD_WRITE",
        __WASI_RIGHT_FD_ADVISE => "__WASI_RIGHT_FD_ADVISE",
        __WASI_RIGHT_FD_ALLOCATE => "__WASI_RIGHT_FD_ALLOCATE",
        __WASI_RIGHT_PATH_CREATE_DIRECTORY => "__WASI_RIGHT_PATH_CREATE_DIRECTORY",
        __WASI_RIGHT_PATH_CREATE_FILE => "__WASI_RIGHT_PATH_CREATE_FILE",
        __WASI_RIGHT_PATH_LINK_SOURCE => "__WASI_RIGHT_PATH_LINK_SOURCE",
        __WASI_RIGHT_PATH_LINK_TARGET => "__WASI_RIGHT_PATH_LINK_TARGET",
        __WASI_RIGHT_PATH_OPEN => "__WASI_RIGHT_PATH_OPEN",
        __WASI_RIGHT_FD_READDIR => "__WASI_RIGHT_FD_READDIR",
        __WASI_RIGHT_PATH_READLINK => "__WASI_RIGHT_PATH_READLINK",
        __WASI_RIGHT_PATH_RENAME_SOURCE => "__WASI_RIGHT_PATH_RENAME_SOURCE",
        __WASI_RIGHT_PATH_RENAME_TARGET => "__WASI_RIGHT_PATH_RENAME_TARGET",
        __WASI_RIGHT_PATH_FILESTAT_GET => "__WASI_RIGHT_PATH_FILESTAT_GET",
        __WASI_RIGHT_PATH_FILESTAT_SET_SIZE => "__WASI_RIGHT_PATH_FILESTAT_SET_SIZE",
        __WASI_RIGHT_PATH_FILESTAT_SET_TIMES => "__WASI_RIGHT_PATH_FILESTAT_SET_TIMES",
        __WASI_RIGHT_FD_FILESTAT_GET => "__WASI_RIGHT_FD_FILESTAT_GET",
        __WASI_RIGHT_FD_FILESTAT_SET_SIZE => "__WASI_RIGHT_FD_FILESTAT_SET_SIZE",
        __WASI_RIGHT_FD_FILESTAT_SET_TIMES => "__WASI_RIGHT_FD_FILESTAT_SET_TIMES",
        __WASI_RIGHT_PATH_SYMLINK => "__WASI_RIGHT_PATH_SYMLINK",
        __WASI_RIGHT_PATH_UNLINK_FILE => "__WASI_RIGHT_PATH_UNLINK_FILE",
        __WASI_RIGHT_PATH_REMOVE_DIRECTORY => "__WASI_RIGHT_PATH_REMOVE_DIRECTORY",
        __WASI_RIGHT_POLL_FD_READWRITE => "__WASI_RIGHT_POLL_FD_READWRITE",
        __WASI_RIGHT_SOCK_SHUTDOWN => "__WASI_RIGHT_SOCK_SHUTDOWN",
        _ => return None,
    })
}

pub type __wasi_riflags_t = u16;
pub const __WASI_SOCK_RECV_PEEK: u16 = 1 << 0;
pub const __WASI_SOCK_RECV_WAITALL: u16 = 1 << 1;

pub type __wasi_roflags_t = u16;
pub const __WASI_SOCK_RECV_DATA_TRUNCATED: u16 = 1 << 0;

pub type __wasi_whence_t = u8;
pub const __WASI_WHENCE_SET: u8 = 0;
pub const __WASI_WHENCE_CUR: u8 = 1;
pub const __WASI_WHENCE_END: u8 = 2;

pub type __wasi_sdflags_t = u8;
pub const __WASI_SHUT_RD: u8 = 1 << 0;
pub const __WASI_SHUT_WR: u8 = 1 << 1;

pub type __wasi_siflags_t = u16;
