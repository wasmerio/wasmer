use crate::*;
use std::{
    fmt,
    mem::{self, MaybeUninit},
};
use wasmer_derive::ValueType;
use wasmer_types::ValueType;
use wasmer_wasi_types_generated::wasi_snapshot0;

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
    pub fs_filetype: wasi_snapshot0::Filetype,
    pub fs_flags: __wasi_fdflags_t,
    pub fs_rights_base: wasi_snapshot0::Rights,
    pub fs_rights_inheriting: wasi_snapshot0::Rights,
}

pub type __wasi_filedelta_t = i64;

pub type __wasi_filesize_t = u64;

#[derive(Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_filestat_t {
    pub st_dev: __wasi_device_t,
    pub st_ino: __wasi_inode_t,
    pub st_filetype: wasi_snapshot0::Filetype,
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
            st_filetype: wasi_snapshot0::Filetype::Unknown,
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
                    self.st_filetype as u8,
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

pub fn wasi_filetype_to_name(ft: wasi_snapshot0::Filetype) -> &'static str {
    match ft {
        wasi_snapshot0::Filetype::Unknown => "Unknown",
        wasi_snapshot0::Filetype::BlockDevice => "Block device",
        wasi_snapshot0::Filetype::CharacterDevice => "Character device",
        wasi_snapshot0::Filetype::Directory => "Directory",
        wasi_snapshot0::Filetype::RegularFile => "Regular file",
        wasi_snapshot0::Filetype::SocketDgram => "Socket dgram",
        wasi_snapshot0::Filetype::SocketStream => "Socket stream",
        wasi_snapshot0::Filetype::SymbolicLink => "Symbolic link",
    }
}

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

/// function for debugging rights issues
#[allow(dead_code)]
pub fn print_right_set(rights: wasi_snapshot0::Rights) {
    // BTreeSet for consistent order
    let mut right_set = std::collections::BTreeSet::new();
    for i in 0..28 {
        let cur_right = rights & wasi_snapshot0::Rights::from_bits(1 << i).unwrap();
        if !cur_right.is_empty() {
            right_set.insert(right_to_string(cur_right).unwrap_or("INVALID RIGHT"));
        }
    }
    println!("{:#?}", right_set);
}

/// expects a single right, returns None if out of bounds or > 1 bit set
pub fn right_to_string(right: wasi_snapshot0::Rights) -> Option<&'static str> {
    Some(match right {
        wasi_snapshot0::Rights::FD_DATASYNC => "Rights::_FD_DATASYNC",
        wasi_snapshot0::Rights::FD_READ => "Rights::FD_READ",
        wasi_snapshot0::Rights::FD_SEEK => "Rights::FD_SEEK",
        wasi_snapshot0::Rights::FD_FDSTAT_SET_FLAGS => "Rights::FD_FDSTAT_SET_FLAGS",
        wasi_snapshot0::Rights::FD_SYNC => "Rights::FD_SYNC",
        wasi_snapshot0::Rights::FD_TELL => "Rights::FD_TELL",
        wasi_snapshot0::Rights::FD_WRITE => "Rights::FD_WRITE",
        wasi_snapshot0::Rights::FD_ADVISE => "Rights::FD_ADVISE",
        wasi_snapshot0::Rights::FD_ALLOCATE => "Rights::FD_ALLOCATE",
        wasi_snapshot0::Rights::PATH_CREATE_DIRECTORY => "Rights::PATH_CREATE_DIRECTORY",
        wasi_snapshot0::Rights::PATH_CREATE_FILE => "Rights::PATH_CREATE_FILE",
        wasi_snapshot0::Rights::PATH_LINK_SOURCE => "Rights::PATH_LINK_SOURCE",
        wasi_snapshot0::Rights::PATH_LINK_TARGET => "Rights::PATH_LINK_TARGET",
        wasi_snapshot0::Rights::PATH_OPEN => "Rights::PATH_OPEN",
        wasi_snapshot0::Rights::FD_READDIR => "Rights::FD_READDIR",
        wasi_snapshot0::Rights::PATH_READLINK => "Rights::PATH_READLINK",
        wasi_snapshot0::Rights::PATH_RENAME_SOURCE => "Rights::PATH_RENAME_SOURCE",
        wasi_snapshot0::Rights::PATH_RENAME_TARGET => "Rights::PATH_RENAME_TARGET",
        wasi_snapshot0::Rights::PATH_FILESTAT_GET => "Rights::PATH_FILESTAT_GET",
        wasi_snapshot0::Rights::PATH_FILESTAT_SET_SIZE => "Rights::PATH_FILESTAT_SET_SIZE",
        wasi_snapshot0::Rights::PATH_FILESTAT_SET_TIMES => "Rights::PATH_FILESTAT_SET_TIMES",
        wasi_snapshot0::Rights::FD_FILESTAT_GET => "Rights::FD_FILESTAT_GET",
        wasi_snapshot0::Rights::FD_FILESTAT_SET_SIZE => "Rights::FD_FILESTAT_SET_SIZE",
        wasi_snapshot0::Rights::FD_FILESTAT_SET_TIMES => "Rights::FD_FILESTAT_SET_TIMES",
        wasi_snapshot0::Rights::PATH_SYMLINK => "Rights::PATH_SYMLINK",
        wasi_snapshot0::Rights::PATH_UNLINK_FILE => "Rights::PATH_UNLINK_FILE",
        wasi_snapshot0::Rights::PATH_REMOVE_DIRECTORY => "Rights::PATH_REMOVE_DIRECTORY",
        wasi_snapshot0::Rights::POLL_FD_READWRITE => "Rights::POLL_FD_READWRITE",
        wasi_snapshot0::Rights::SOCK_SHUTDOWN => "Rights::SOCK_SHUTDOWN",
        _ => return None,
    })
}

pub type __wasi_whence_t = u8;
pub const __WASI_WHENCE_SET: __wasi_whence_t = 0;
pub const __WASI_WHENCE_CUR: __wasi_whence_t = 1;
pub const __WASI_WHENCE_END: __wasi_whence_t = 2;
