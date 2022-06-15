use crate::*;
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    mem::{self, MaybeUninit},
};
use wasmer_derive::ValueType;
use wasmer_types::ValueType;

pub type __wasi_device_t = u64;

pub type __wasi_fd_t = u32;
pub const __WASI_STDIN_FILENO: __wasi_fd_t = 0;
pub const __WASI_STDOUT_FILENO: __wasi_fd_t = 1;
pub const __WASI_STDERR_FILENO: __wasi_fd_t = 2;

pub type __wasi_pid_t = u32;
pub type __wasi_tid_t = u32;

pub type __wasi_fdflags_t = u16;
pub const __WASI_FDFLAG_APPEND: __wasi_fdflags_t = 1 << 0;
pub const __WASI_FDFLAG_DSYNC: __wasi_fdflags_t = 1 << 1;
pub const __WASI_FDFLAG_NONBLOCK: __wasi_fdflags_t = 1 << 2;
pub const __WASI_FDFLAG_RSYNC: __wasi_fdflags_t = 1 << 3;
pub const __WASI_FDFLAG_SYNC: __wasi_fdflags_t = 1 << 4;

pub type __wasi_eventfdflags = u16;
pub const __WASI_EVENTFDFLAGS_SEMAPHORE: __wasi_eventfdflags = 1 << 0;

pub type __wasi_preopentype_t = u8;
pub const __WASI_PREOPENTYPE_DIR: __wasi_preopentype_t = 0;

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_prestat_u_dir_t {
    pub pr_name_len: u32,
}

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

unsafe impl ValueType for __wasi_prestat_t {
    fn zero_padding_bytes(&self, bytes: &mut [MaybeUninit<u8>]) {
        macro_rules! field {
            ($($f:tt)*) => {
                &self.$($f)* as *const _ as usize - self as *const _ as usize
            };
        }
        macro_rules! field_end {
            ($($f:tt)*) => {
                field!($($f)*) + mem::size_of_val(&self.$($f)*)
            };
        }
        macro_rules! zero {
            ($start:expr, $end:expr) => {
                for i in $start..$end {
                    bytes[i] = MaybeUninit::new(0);
                }
            };
        }
        self.pr_type
            .zero_padding_bytes(&mut bytes[field!(pr_type)..field_end!(pr_type)]);
        zero!(field_end!(pr_type), field!(u));
        match self.pr_type {
            __WASI_PREOPENTYPE_DIR => unsafe {
                self.u
                    .dir
                    .zero_padding_bytes(&mut bytes[field!(u.dir)..field_end!(u.dir)]);
                zero!(field_end!(u.dir), field_end!(u));
            },
            _ => zero!(field!(u), field_end!(u)),
        }
        zero!(field_end!(u), mem::size_of_val(self));
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_fdstat_t {
    pub fs_filetype: __wasi_filetype_t,
    pub fs_flags: __wasi_fdflags_t,
    pub fs_rights_base: __wasi_rights_t,
    pub fs_rights_inheriting: __wasi_rights_t,
}

pub type __wasi_filedelta_t = i64;

pub type __wasi_filesize_t = u64;

#[derive(Copy, Clone, PartialEq, Eq, ValueType)]
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
pub const __WASI_FILETYPE_UNKNOWN: __wasi_filetype_t = 0;
pub const __WASI_FILETYPE_BLOCK_DEVICE: __wasi_filetype_t = 1;
pub const __WASI_FILETYPE_CHARACTER_DEVICE: __wasi_filetype_t = 2;
pub const __WASI_FILETYPE_DIRECTORY: __wasi_filetype_t = 3;
pub const __WASI_FILETYPE_REGULAR_FILE: __wasi_filetype_t = 4;
pub const __WASI_FILETYPE_SOCKET_DGRAM: __wasi_filetype_t = 5;
pub const __WASI_FILETYPE_SOCKET_STREAM: __wasi_filetype_t = 6;
pub const __WASI_FILETYPE_SYMBOLIC_LINK: __wasi_filetype_t = 7;
pub const __WASI_FILETYPE_SOCKET_RAW: __wasi_filetype_t = 8;
pub const __WASI_FILETYPE_SOCKET_SEQPACKET: __wasi_filetype_t = 9;

pub type __wasi_fstflags_t = u16;
pub const __WASI_FILESTAT_SET_ATIM: __wasi_fstflags_t = 1 << 0;
pub const __WASI_FILESTAT_SET_ATIM_NOW: __wasi_fstflags_t = 1 << 1;
pub const __WASI_FILESTAT_SET_MTIM: __wasi_fstflags_t = 1 << 2;
pub const __WASI_FILESTAT_SET_MTIM_NOW: __wasi_fstflags_t = 1 << 3;

pub type __wasi_inode_t = u64;

pub type __wasi_linkcount_t = u64;

pub type __wasi_lookupflags_t = u32;
pub const __WASI_LOOKUP_SYMLINK_FOLLOW: __wasi_lookupflags_t = 1 << 0;

pub type __wasi_oflags_t = u16;
pub const __WASI_O_CREAT: __wasi_oflags_t = 1 << 0;
pub const __WASI_O_DIRECTORY: __wasi_oflags_t = 1 << 1;
pub const __WASI_O_EXCL: __wasi_oflags_t = 1 << 2;
pub const __WASI_O_TRUNC: __wasi_oflags_t = 1 << 3;

pub type __wasi_rights_t = u64;
pub const __WASI_RIGHT_FD_DATASYNC: __wasi_rights_t = 1 << 0;
pub const __WASI_RIGHT_FD_READ: __wasi_rights_t = 1 << 1;
pub const __WASI_RIGHT_FD_SEEK: __wasi_rights_t = 1 << 2;
pub const __WASI_RIGHT_FD_FDSTAT_SET_FLAGS: __wasi_rights_t = 1 << 3;
pub const __WASI_RIGHT_FD_SYNC: __wasi_rights_t = 1 << 4;
pub const __WASI_RIGHT_FD_TELL: __wasi_rights_t = 1 << 5;
pub const __WASI_RIGHT_FD_WRITE: __wasi_rights_t = 1 << 6;
pub const __WASI_RIGHT_FD_ADVISE: __wasi_rights_t = 1 << 7;
pub const __WASI_RIGHT_FD_ALLOCATE: __wasi_rights_t = 1 << 8;
pub const __WASI_RIGHT_PATH_CREATE_DIRECTORY: __wasi_rights_t = 1 << 9;
pub const __WASI_RIGHT_PATH_CREATE_FILE: __wasi_rights_t = 1 << 10;
pub const __WASI_RIGHT_PATH_LINK_SOURCE: __wasi_rights_t = 1 << 11;
pub const __WASI_RIGHT_PATH_LINK_TARGET: __wasi_rights_t = 1 << 12;
pub const __WASI_RIGHT_PATH_OPEN: __wasi_rights_t = 1 << 13;
pub const __WASI_RIGHT_FD_READDIR: __wasi_rights_t = 1 << 14;
pub const __WASI_RIGHT_PATH_READLINK: __wasi_rights_t = 1 << 15;
pub const __WASI_RIGHT_PATH_RENAME_SOURCE: __wasi_rights_t = 1 << 16;
pub const __WASI_RIGHT_PATH_RENAME_TARGET: __wasi_rights_t = 1 << 17;
pub const __WASI_RIGHT_PATH_FILESTAT_GET: __wasi_rights_t = 1 << 18;
pub const __WASI_RIGHT_PATH_FILESTAT_SET_SIZE: __wasi_rights_t = 1 << 19;
pub const __WASI_RIGHT_PATH_FILESTAT_SET_TIMES: __wasi_rights_t = 1 << 20;
pub const __WASI_RIGHT_FD_FILESTAT_GET: __wasi_rights_t = 1 << 21;
pub const __WASI_RIGHT_FD_FILESTAT_SET_SIZE: __wasi_rights_t = 1 << 22;
pub const __WASI_RIGHT_FD_FILESTAT_SET_TIMES: __wasi_rights_t = 1 << 23;
pub const __WASI_RIGHT_PATH_SYMLINK: __wasi_rights_t = 1 << 24;
pub const __WASI_RIGHT_PATH_REMOVE_DIRECTORY: __wasi_rights_t = 1 << 25;
pub const __WASI_RIGHT_PATH_UNLINK_FILE: __wasi_rights_t = 1 << 26;
pub const __WASI_RIGHT_POLL_FD_READWRITE: __wasi_rights_t = 1 << 27;
pub const __WASI_RIGHT_SOCK_SHUTDOWN: __wasi_rights_t = 1 << 28;
pub const __WASI_RIGHT_SOCK_ACCEPT: __wasi_rights_t = 1 << 29;
pub const __WASI_RIGHT_SOCK_CONNECT: __wasi_rights_t = 1 << 30;
pub const __WASI_RIGHT_SOCK_LISTEN: __wasi_rights_t = 1 << 31;
pub const __WASI_RIGHT_SOCK_BIND: __wasi_rights_t = 1 << 32;
pub const __WASI_RIGHT_SOCK_RECV: __wasi_rights_t = 1 << 33;
pub const __WASI_RIGHT_SOCK_SEND: __wasi_rights_t = 1 << 34;
pub const __WASI_RIGHT_SOCK_ADDR_LOCAL: __wasi_rights_t = 1 << 35;
pub const __WASI_RIGHT_SOCK_ADDR_REMOTE: __wasi_rights_t = 1 << 36;
pub const __WASI_RIGHT_SOCK_RECV_FROM: __wasi_rights_t = 1 << 37;
pub const __WASI_RIGHT_SOCK_SEND_TO: __wasi_rights_t = 1 << 38;

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

pub type __wasi_whence_t = u8;
pub const __WASI_WHENCE_SET: __wasi_whence_t = 0;
pub const __WASI_WHENCE_CUR: __wasi_whence_t = 1;
pub const __WASI_WHENCE_END: __wasi_whence_t = 2;
