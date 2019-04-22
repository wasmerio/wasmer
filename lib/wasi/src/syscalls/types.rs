#![allow(non_camel_case_types)]

use crate::ptr::{Array, WasmPtr};
use byteorder::{ReadBytesExt, WriteBytesExt, LE};
use std::fmt;
use std::mem;
use wasmer_runtime_core::types::ValueType;

pub type __wasi_advice_t = u8;
pub const __WASI_ADVICE_DONTNEED: u8 = 0;
pub const __WASI_ADVICE_NOREUSE: u8 = 1;
pub const __WASI_ADVICE_NORMAL: u8 = 2;
pub const __WASI_ADVICE_RANDOM: u8 = 3;
pub const __WASI_ADVICE_SEQUENTIAL: u8 = 4;
pub const __WASI_ADVICE_WILLNEED: u8 = 5;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct __wasi_ciovec_t {
    pub buf: WasmPtr<u8, Array>,
    pub buf_len: u32,
}

unsafe impl ValueType for __wasi_ciovec_t {}

pub type __wasi_clockid_t = u32;
pub const __WASI_CLOCK_MONOTONIC: u32 = 0;
pub const __WASI_CLOCK_PROCESS_CPUTIME_ID: u32 = 1;
pub const __WASI_CLOCK_REALTIME: u32 = 2;
pub const __WASI_CLOCK_THREAD_CPUTIME_ID: u32 = 3;

pub type __wasi_device_t = u64;

pub type __wasi_dircookie_t = u64;
pub const __WASI_DIRCOOKIE_START: u64 = 0;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct __wasi_dirent_t {
    pub d_next: __wasi_dircookie_t,
    pub d_ino: __wasi_inode_t,
    pub d_namlen: u32,
    pub d_type: __wasi_filetype_t,
}

pub type __wasi_errno_t = u32;
pub const __WASI_ESUCCESS: u32 = 0;
pub const __WASI_E2BIG: u32 = 1;
pub const __WASI_EACCES: u32 = 2;
pub const __WASI_EADDRINUSE: u32 = 3;
pub const __WASI_EADDRNOTAVAIL: u32 = 4;
pub const __WASI_EAFNOSUPPORT: u32 = 5;
pub const __WASI_EAGAIN: u32 = 6;
pub const __WASI_EALREADY: u32 = 7;
pub const __WASI_EBADF: u32 = 8;
pub const __WASI_EBADMSG: u32 = 9;
pub const __WASI_EBUSY: u32 = 10;
pub const __WASI_ECANCELED: u32 = 11;
pub const __WASI_ECHILD: u32 = 12;
pub const __WASI_ECONNABORTED: u32 = 13;
pub const __WASI_ECONNREFUSED: u32 = 14;
pub const __WASI_ECONNRESET: u32 = 15;
pub const __WASI_EDEADLK: u32 = 16;
pub const __WASI_EDESTADDRREQ: u32 = 17;
pub const __WASI_EDOM: u32 = 18;
pub const __WASI_EDQUOT: u32 = 19;
pub const __WASI_EEXIST: u32 = 20;
pub const __WASI_EFAULT: u32 = 21;
pub const __WASI_EFBIG: u32 = 22;
pub const __WASI_EHOSTUNREACH: u32 = 23;
pub const __WASI_EIDRM: u32 = 24;
pub const __WASI_EILSEQ: u32 = 25;
pub const __WASI_EINPROGRESS: u32 = 26;
pub const __WASI_EINTR: u32 = 27;
pub const __WASI_EINVAL: u32 = 28;
pub const __WASI_EIO: u32 = 29;
pub const __WASI_EISCONN: u32 = 30;
pub const __WASI_EISDIR: u32 = 31;
pub const __WASI_ELOOP: u32 = 32;
pub const __WASI_EMFILE: u32 = 33;
pub const __WASI_EMLINK: u32 = 34;
pub const __WASI_EMSGSIZE: u32 = 35;
pub const __WASI_EMULTIHOP: u32 = 36;
pub const __WASI_ENAMETOOLONG: u32 = 37;
pub const __WASI_ENETDOWN: u32 = 38;
pub const __WASI_ENETRESET: u32 = 39;
pub const __WASI_ENETUNREACH: u32 = 40;
pub const __WASI_ENFILE: u32 = 41;
pub const __WASI_ENOBUFS: u32 = 42;
pub const __WASI_ENODEV: u32 = 43;
pub const __WASI_ENOENT: u32 = 44;
pub const __WASI_ENOEXEC: u32 = 45;
pub const __WASI_ENOLCK: u32 = 46;
pub const __WASI_ENOLINK: u32 = 47;
pub const __WASI_ENOMEM: u32 = 48;
pub const __WASI_ENOMSG: u32 = 49;
pub const __WASI_ENOPROTOOPT: u32 = 50;
pub const __WASI_ENOSPC: u32 = 51;
pub const __WASI_ENOSYS: u32 = 52;
pub const __WASI_ENOTCONN: u32 = 53;
pub const __WASI_ENOTDIR: u32 = 54;
pub const __WASI_ENOTEMPTY: u32 = 55;
pub const __WASI_ENOTRECOVERABLE: u32 = 56;
pub const __WASI_ENOTSOCK: u32 = 57;
pub const __WASI_ENOTSUP: u32 = 58;
pub const __WASI_ENOTTY: u32 = 59;
pub const __WASI_ENXIO: u32 = 60;
pub const __WASI_EOVERFLOW: u32 = 61;
pub const __WASI_EOWNERDEAD: u32 = 62;
pub const __WASI_EPERM: u32 = 63;
pub const __WASI_EPIPE: u32 = 64;
pub const __WASI_EPROTO: u32 = 65;
pub const __WASI_EPROTONOSUPPORT: u32 = 66;
pub const __WASI_EPROTOTYPE: u32 = 67;
pub const __WASI_ERANGE: u32 = 68;
pub const __WASI_EROFS: u32 = 69;
pub const __WASI_ESPIPE: u32 = 70;
pub const __WASI_ESRCH: u32 = 71;
pub const __WASI_ESTALE: u32 = 72;
pub const __WASI_ETIMEDOUT: u32 = 73;
pub const __WASI_ETXTBSY: u32 = 74;
pub const __WASI_EXDEV: u32 = 75;
pub const __WASI_ENOTCAPABLE: u32 = 76;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct __wasi_event_fd_readwrite_t {
    pub nbytes: __wasi_filesize_t,
    pub flags: __wasi_eventrwflags_t,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub union __wasi_event_u {
    fd_readwrite: __wasi_event_fd_readwrite_t,
}

#[derive(Copy, Clone)]
pub enum EventEnum {
    FdReadWrite {
        nbytes: __wasi_filesize_t,
        flags: __wasi_eventrwflags_t,
    },
}

impl EventEnum {
    pub fn untagged(self) -> __wasi_event_u {
        match self {
            EventEnum::FdReadWrite { nbytes, flags } => __wasi_event_u {
                fd_readwrite: __wasi_event_fd_readwrite_t { nbytes, flags },
            },
        }
    }
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct __wasi_event_t {
    pub userdata: __wasi_userdata_t,
    pub error: __wasi_errno_t,
    pub type_: __wasi_eventtype_t,
    pub u: __wasi_event_u,
}

impl __wasi_event_t {
    pub fn tagged(&self) -> Option<EventEnum> {
        match self.type_ {
            __WASI_EVENTTYPE_FD_READ | __WASI_EVENTTYPE_FD_WRITE => Some(EventEnum::FdReadWrite {
                nbytes: unsafe { self.u.fd_readwrite.nbytes },
                flags: unsafe { self.u.fd_readwrite.flags },
            }),
            _ => None,
        }
    }
}

pub type __wasi_eventrwflags_t = u32;
pub const __WASI_EVENT_FD_READWRITE_HANGUP: u32 = 1 << 0;

pub type __wasi_eventtype_t = u8;
pub const __WASI_EVENTTYPE_CLOCK: u8 = 0;
pub const __WASI_EVENTTYPE_FD_READ: u8 = 1;
pub const __WASI_EVENTTYPE_FD_WRITE: u8 = 2;

pub type __wasi_exitcode_t = u32;

pub type __wasi_fd_t = u32;
pub const __WASI_STDIN_FILENO: u32 = 0;
pub const __WASI_STDOUT_FILENO: u32 = 1;
pub const __WASI_STDERR_FILENO: u32 = 2;

pub type __wasi_fdflags_t = u32;
pub const __WASI_FDFLAG_APPEND: u32 = 1 << 0;
pub const __WASI_FDFLAG_DSYNC: u32 = 1 << 1;
pub const __WASI_FDFLAG_NONBLOCK: u32 = 1 << 2;
pub const __WASI_FDFLAG_RSYNC: u32 = 1 << 3;
pub const __WASI_FDFLAG_SYNC: u32 = 1 << 4;

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

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
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

unsafe impl ValueType for __wasi_filestat_t {}

pub type __wasi_filetype_t = u8;
pub const __WASI_FILETYPE_UNKNOWN: u8 = 0;
pub const __WASI_FILETYPE_BLOCK_DEVICE: u8 = 1;
pub const __WASI_FILETYPE_CHARACTER_DEVICE: u8 = 2;
pub const __WASI_FILETYPE_DIRECTORY: u8 = 3;
pub const __WASI_FILETYPE_REGULAR_FILE: u8 = 4;
pub const __WASI_FILETYPE_SOCKET_DGRAM: u8 = 5;
pub const __WASI_FILETYPE_SOCKET_STREAM: u8 = 6;
pub const __WASI_FILETYPE_SYMBOLIC_LINK: u8 = 7;

pub type __wasi_fstflags_t = u32;
pub const __WASI_FILESTAT_SET_ATIM: u32 = 1 << 0;
pub const __WASI_FILESTAT_SET_ATIM_NOW: u32 = 1 << 1;
pub const __WASI_FILESTAT_SET_MTIM: u32 = 1 << 2;
pub const __WASI_FILESTAT_SET_MTIM_NOW: u32 = 1 << 3;

pub type __wasi_inode_t = u64;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct __wasi_iovec_t {
    pub buf: WasmPtr<u8, Array>,
    pub buf_len: u32,
}

unsafe impl ValueType for __wasi_iovec_t {}

pub type __wasi_linkcount_t = u32;

pub type __wasi_lookupflags_t = u32;
pub const __WASI_LOOKUP_SYMLINK_FOLLOW: u32 = 1 << 0;

pub type __wasi_oflags_t = u32;
pub const __WASI_O_CREAT: u32 = 1 << 0;
pub const __WASI_O_DIRECTORY: u32 = 1 << 1;
pub const __WASI_O_EXCL: u32 = 1 << 2;
pub const __WASI_O_TRUNC: u32 = 1 << 3;

pub type __wasi_riflags_t = u32;
pub const __WASI_SOCK_RECV_PEEK: u32 = 1 << 0;
pub const __WASI_SOCK_RECV_WAITALL: u32 = 1 << 1;

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
pub const __WASI_RIGHT_PATH_UNLINK_FILE: u64 = 1 << 25;
pub const __WASI_RIGHT_PATH_REMOVE_DIRECTORY: u64 = 1 << 26;
pub const __WASI_RIGHT_POLL_FD_READWRITE: u64 = 1 << 27;
pub const __WASI_RIGHT_SOCK_SHUTDOWN: u64 = 1 << 28;

pub type __wasi_roflags_t = u32;
pub const __WASI_SOCK_RECV_DATA_TRUNCATED: u32 = 1 << 0;

pub type __wasi_sdflags_t = u8;
pub const __WASI_SHUT_RD: u8 = 1 << 0;
pub const __WASI_SHUT_WR: u8 = 1 << 1;

pub type __wasi_siflags_t = u32;

pub type __wasi_signal_t = u8;
pub const __WASI_SIGABRT: u8 = 0;
pub const __WASI_SIGALRM: u8 = 1;
pub const __WASI_SIGBUS: u8 = 2;
pub const __WASI_SIGCHLD: u8 = 3;
pub const __WASI_SIGCONT: u8 = 4;
pub const __WASI_SIGFPE: u8 = 5;
pub const __WASI_SIGHUP: u8 = 6;
pub const __WASI_SIGILL: u8 = 7;
pub const __WASI_SIGINT: u8 = 8;
pub const __WASI_SIGKILL: u8 = 9;
pub const __WASI_SIGPIPE: u8 = 10;
pub const __WASI_SIGQUIT: u8 = 11;
pub const __WASI_SIGSEGV: u8 = 12;
pub const __WASI_SIGSTOP: u8 = 13;
pub const __WASI_SIGSYS: u8 = 14;
pub const __WASI_SIGTERM: u8 = 15;
pub const __WASI_SIGTRAP: u8 = 16;
pub const __WASI_SIGTSTP: u8 = 17;
pub const __WASI_SIGTTIN: u8 = 18;
pub const __WASI_SIGTTOU: u8 = 19;
pub const __WASI_SIGURG: u8 = 20;
pub const __WASI_SIGUSR1: u8 = 21;
pub const __WASI_SIGUSR2: u8 = 22;
pub const __WASI_SIGVTALRM: u8 = 23;
pub const __WASI_SIGXCPU: u8 = 24;
pub const __WASI_SIGXFSZ: u8 = 25;

pub type __wasi_subclockflags_t = u32;
pub const __WASI_SUBSCRIPTION_CLOCK_ABSTIME: u32 = 1 << 0;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct __wasi_subscription_clock_t {
    pub userdata: __wasi_userdata_t,
    pub clock_id: __wasi_clockid_t,
    pub timeout: __wasi_timestamp_t,
    pub precision: __wasi_timestamp_t,
    pub flags: __wasi_subclockflags_t,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct __wasi_subscription_fs_readwrite_t {
    pub fd: __wasi_fd_t,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub union __wasi_subscription_u {
    clock: __wasi_subscription_clock_t,
    fd_readwrite: __wasi_subscription_fs_readwrite_t,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct __wasi_subscription_t {
    pub userdata: __wasi_userdata_t,
    pub type_: __wasi_eventtype_t,
    pub u: __wasi_subscription_u,
}

pub enum SubscriptionEnum {
    Clock(__wasi_subscription_clock_t),
    FdReadWrite(__wasi_subscription_fs_readwrite_t),
}

impl __wasi_subscription_t {
    pub fn tagged(&self) -> Option<SubscriptionEnum> {
        match self.type_ {
            __WASI_EVENTTYPE_CLOCK => Some(SubscriptionEnum::Clock(unsafe { self.u.clock })),
            __WASI_EVENTTYPE_FD_READ | __WASI_EVENTTYPE_FD_WRITE => {
                Some(SubscriptionEnum::FdReadWrite(unsafe {
                    self.u.fd_readwrite
                }))
            }
            _ => None,
        }
    }
}

pub type __wasi_timestamp_t = u64;

pub type __wasi_userdata_t = u64;

pub type __wasi_whence_t = u8;
pub const __WASI_WHENCE_CUR: u8 = 0;
pub const __WASI_WHENCE_END: u8 = 1;
pub const __WASI_WHENCE_SET: u8 = 2;
