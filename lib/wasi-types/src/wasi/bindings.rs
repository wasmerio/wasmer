use num_enum::{IntoPrimitive, TryFromPrimitive};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use std::mem::MaybeUninit;
use wasmer::{MemorySize, ValueType};
// TODO: Remove once bindings generate wai_bindgen_rust::bitflags::bitflags!  (temp hack)
use wai_bindgen_rust as wit_bindgen_rust;

use super::ExitCode;

#[doc = " Type names used by low-level WASI interfaces."]
#[doc = " An array size."]
#[doc = " "]
#[doc = " Note: This is similar to `size_t` in POSIX."]
pub type Size = u32;
#[doc = " Non-negative file size or length of a region within a file."]
pub type Filesize = u64;
#[doc = " Timestamp in nanoseconds."]
pub type Timestamp = u64;
#[doc = " A file descriptor handle."]
pub type Fd = u32;
#[doc = " A reference to the offset of a directory entry."]
pub type Dircookie = u64;
#[doc = " The type for the `dirent::d-namlen` field of `dirent` struct."]
pub type Dirnamlen = u32;
#[doc = " File serial number that is unique within its file system."]
pub type Inode = u64;
#[doc = " Identifier for a device containing a file system. Can be used in combination"]
#[doc = " with `inode` to uniquely identify a file or directory in the filesystem."]
pub type Device = u64;
pub type Linkcount = u64;
pub type Snapshot0Linkcount = u32;
pub type Tid = u32;
pub type Pid = u32;
#[doc = " Identifiers for clocks, snapshot0 version."]
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, num_enum :: TryFromPrimitive, Hash)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub enum Snapshot0Clockid {
    #[doc = " The clock measuring real time. Time value zero corresponds with"]
    #[doc = " 1970-01-01T00:00:00Z."]
    Realtime,
    #[doc = " The store-wide monotonic clock, which is defined as a clock measuring"]
    #[doc = " real time, whose value cannot be adjusted and which cannot have negative"]
    #[doc = " clock jumps. The epoch of this clock is undefined. The absolute time"]
    #[doc = " value of this clock therefore has no meaning."]
    Monotonic,
    #[doc = " The CPU-time clock associated with the current process."]
    ProcessCputimeId,
    #[doc = " The CPU-time clock associated with the current thread."]
    ThreadCputimeId,
    #[doc = " The clock type is not known."]
    Unknown = 255,
}
impl core::fmt::Debug for Snapshot0Clockid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Snapshot0Clockid::Realtime => f.debug_tuple("Snapshot0Clockid::Realtime").finish(),
            Snapshot0Clockid::Monotonic => f.debug_tuple("Snapshot0Clockid::Monotonic").finish(),
            Snapshot0Clockid::ProcessCputimeId => {
                f.debug_tuple("Snapshot0Clockid::ProcessCputimeId").finish()
            }
            Snapshot0Clockid::ThreadCputimeId => {
                f.debug_tuple("Snapshot0Clockid::ThreadCputimeId").finish()
            }
            Snapshot0Clockid::Unknown => f.debug_tuple("Snapshot0Clockid::Unknown").finish(),
        }
    }
}
#[doc = " Identifiers for clocks."]
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, num_enum :: TryFromPrimitive, Hash)]
pub enum Clockid {
    #[doc = " The clock measuring real time. Time value zero corresponds with"]
    #[doc = " 1970-01-01T00:00:00Z."]
    Realtime,
    #[doc = " The store-wide monotonic clock, which is defined as a clock measuring"]
    #[doc = " real time, whose value cannot be adjusted and which cannot have negative"]
    #[doc = " clock jumps. The epoch of this clock is undefined. The absolute time"]
    #[doc = " value of this clock therefore has no meaning."]
    Monotonic,
    #[doc = " The CPU-time clock associated with the current process."]
    ProcessCputimeId,
    #[doc = " The CPU-time clock associated with the current thread."]
    ThreadCputimeId,
    #[doc = " The clock type is unknown."]
    Unknown = 255,
}
impl core::fmt::Debug for Clockid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Clockid::Realtime => f.debug_tuple("Clockid::Realtime").finish(),
            Clockid::Monotonic => f.debug_tuple("Clockid::Monotonic").finish(),
            Clockid::ProcessCputimeId => f.debug_tuple("Clockid::ProcessCputimeId").finish(),
            Clockid::ThreadCputimeId => f.debug_tuple("Clockid::ThreadCputimeId").finish(),
            Clockid::Unknown => f.debug_tuple("Clockid::Unknown").finish(),
        }
    }
}
#[doc = " Error codes returned by functions."]
#[doc = " Not all of these error codes are returned by the functions provided by this"]
#[doc = " API; some are used in higher-level library layers, and others are provided"]
#[doc = " merely for alignment with POSIX."]
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, IntoPrimitive, TryFromPrimitive)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub enum Errno {
    #[doc = " No error occurred. System call completed successfully."]
    Success,
    #[doc = " Argument list too long."]
    Toobig,
    #[doc = " Permission denied."]
    Access,
    #[doc = " Address in use."]
    Addrinuse,
    #[doc = " Address not available."]
    Addrnotavail,
    #[doc = " Address family not supported."]
    Afnosupport,
    #[doc = " Resource unavailable, or operation would block."]
    Again,
    #[doc = " Connection already in progress."]
    Already,
    #[doc = " Bad file descriptor."]
    Badf,
    #[doc = " Bad message."]
    Badmsg,
    #[doc = " Device or resource busy."]
    Busy,
    #[doc = " Operation canceled."]
    Canceled,
    #[doc = " No child processes."]
    Child,
    #[doc = " Connection aborted."]
    Connaborted,
    #[doc = " Connection refused."]
    Connrefused,
    #[doc = " Connection reset."]
    Connreset,
    #[doc = " Resource deadlock would occur."]
    Deadlk,
    #[doc = " Destination address required."]
    Destaddrreq,
    #[doc = " Mathematics argument out of domain of function."]
    Dom,
    #[doc = " Reserved."]
    Dquot,
    #[doc = " File exists."]
    Exist,
    #[doc = " Bad address."]
    Fault,
    #[doc = " File too large."]
    Fbig,
    #[doc = " Host is unreachable."]
    Hostunreach,
    #[doc = " Identifier removed."]
    Idrm,
    #[doc = " Illegal byte sequence."]
    Ilseq,
    #[doc = " Operation in progress."]
    Inprogress,
    #[doc = " Interrupted function."]
    Intr,
    #[doc = " Invalid argument."]
    Inval,
    #[doc = " I/O error."]
    Io,
    #[doc = " Socket is connected."]
    Isconn,
    #[doc = " Is a directory."]
    Isdir,
    #[doc = " Too many levels of symbolic links."]
    Loop,
    #[doc = " File descriptor value too large."]
    Mfile,
    #[doc = " Too many links."]
    Mlink,
    #[doc = " Message too large."]
    Msgsize,
    #[doc = " Reserved."]
    Multihop,
    #[doc = " Filename too long."]
    Nametoolong,
    #[doc = " Network is down."]
    Netdown,
    #[doc = " Connection aborted by network."]
    Netreset,
    #[doc = " Network unreachable."]
    Netunreach,
    #[doc = " Too many files open in system."]
    Nfile,
    #[doc = " No buffer space available."]
    Nobufs,
    #[doc = " No such device."]
    Nodev,
    #[doc = " No such file or directory."]
    Noent,
    #[doc = " Executable file format error."]
    Noexec,
    #[doc = " No locks available."]
    Nolck,
    #[doc = " Reserved."]
    Nolink,
    #[doc = " Not enough space."]
    Nomem,
    #[doc = " No message of the desired type."]
    Nomsg,
    #[doc = " Protocol not available."]
    Noprotoopt,
    #[doc = " No space left on device."]
    Nospc,
    #[doc = " Function not supported."]
    Nosys,
    #[doc = " The socket is not connected."]
    Notconn,
    #[doc = " Not a directory or a symbolic link to a directory."]
    Notdir,
    #[doc = " Directory not empty."]
    Notempty,
    #[doc = " State not recoverable."]
    Notrecoverable,
    #[doc = " Not a socket."]
    Notsock,
    #[doc = " Not supported, or operation not supported on socket."]
    Notsup,
    #[doc = " Inappropriate I/O control operation."]
    Notty,
    #[doc = " No such device or address."]
    Nxio,
    #[doc = " Value too large to be stored in data type."]
    Overflow,
    #[doc = " Previous owner died."]
    Ownerdead,
    #[doc = " Operation not permitted."]
    Perm,
    #[doc = " Broken pipe."]
    Pipe,
    #[doc = " Protocol error."]
    Proto,
    #[doc = " Protocol not supported."]
    Protonosupport,
    #[doc = " Protocol wrong type for socket."]
    Prototype,
    #[doc = " Result too large."]
    Range,
    #[doc = " Read-only file system."]
    Rofs,
    #[doc = " Invalid seek."]
    Spipe,
    #[doc = " No such process."]
    Srch,
    #[doc = " Reserved."]
    Stale,
    #[doc = " Connection timed out."]
    Timedout,
    #[doc = " Text file busy."]
    Txtbsy,
    #[doc = " Cross-device link."]
    Xdev,
    #[doc = " Extension: Capabilities insufficient."]
    Notcapable,
    #[doc = " Cannot send after socket shutdown."]
    Shutdown,
    #[doc = " Memory access violation."]
    Memviolation,
    #[doc = " An unknown error has occured"]
    Unknown,
}
impl Errno {
    pub fn name(&self) -> &'static str {
        match self {
            Errno::Success => "success",
            Errno::Toobig => "toobig",
            Errno::Access => "access",
            Errno::Addrinuse => "addrinuse",
            Errno::Addrnotavail => "addrnotavail",
            Errno::Afnosupport => "afnosupport",
            Errno::Again => "again",
            Errno::Already => "already",
            Errno::Badf => "badf",
            Errno::Badmsg => "badmsg",
            Errno::Busy => "busy",
            Errno::Canceled => "canceled",
            Errno::Child => "child",
            Errno::Connaborted => "connaborted",
            Errno::Connrefused => "connrefused",
            Errno::Connreset => "connreset",
            Errno::Deadlk => "deadlk",
            Errno::Destaddrreq => "destaddrreq",
            Errno::Dom => "dom",
            Errno::Dquot => "dquot",
            Errno::Exist => "exist",
            Errno::Fault => "fault",
            Errno::Fbig => "fbig",
            Errno::Hostunreach => "hostunreach",
            Errno::Idrm => "idrm",
            Errno::Ilseq => "ilseq",
            Errno::Inprogress => "inprogress",
            Errno::Intr => "intr",
            Errno::Inval => "inval",
            Errno::Io => "io",
            Errno::Isconn => "isconn",
            Errno::Isdir => "isdir",
            Errno::Loop => "loop",
            Errno::Mfile => "mfile",
            Errno::Mlink => "mlink",
            Errno::Msgsize => "msgsize",
            Errno::Multihop => "multihop",
            Errno::Nametoolong => "nametoolong",
            Errno::Netdown => "netdown",
            Errno::Netreset => "netreset",
            Errno::Netunreach => "netunreach",
            Errno::Nfile => "nfile",
            Errno::Nobufs => "nobufs",
            Errno::Nodev => "nodev",
            Errno::Noent => "noent",
            Errno::Noexec => "noexec",
            Errno::Nolck => "nolck",
            Errno::Nolink => "nolink",
            Errno::Nomem => "nomem",
            Errno::Nomsg => "nomsg",
            Errno::Noprotoopt => "noprotoopt",
            Errno::Nospc => "nospc",
            Errno::Nosys => "nosys",
            Errno::Notconn => "notconn",
            Errno::Notdir => "notdir",
            Errno::Notempty => "notempty",
            Errno::Notrecoverable => "notrecoverable",
            Errno::Notsock => "notsock",
            Errno::Notsup => "notsup",
            Errno::Notty => "notty",
            Errno::Nxio => "nxio",
            Errno::Overflow => "overflow",
            Errno::Ownerdead => "ownerdead",
            Errno::Perm => "perm",
            Errno::Pipe => "pipe",
            Errno::Proto => "proto",
            Errno::Protonosupport => "protonosupport",
            Errno::Prototype => "prototype",
            Errno::Range => "range",
            Errno::Rofs => "rofs",
            Errno::Spipe => "spipe",
            Errno::Srch => "srch",
            Errno::Stale => "stale",
            Errno::Timedout => "timedout",
            Errno::Txtbsy => "txtbsy",
            Errno::Xdev => "xdev",
            Errno::Notcapable => "notcapable",
            Errno::Shutdown => "shutdown",
            Errno::Memviolation => "memviolation",
            Errno::Unknown => "unknown",
        }
    }
    pub fn message(&self) -> &'static str {
        match self {
            Errno::Success => "No error occurred. System call completed successfully.",
            Errno::Toobig => "Argument list too long.",
            Errno::Access => "Permission denied.",
            Errno::Addrinuse => "Address in use.",
            Errno::Addrnotavail => "Address not available.",
            Errno::Afnosupport => "Address family not supported.",
            Errno::Again => "Resource unavailable, or operation would block.",
            Errno::Already => "Connection already in progress.",
            Errno::Badf => "Bad file descriptor.",
            Errno::Badmsg => "Bad message.",
            Errno::Busy => "Device or resource busy.",
            Errno::Canceled => "Operation canceled.",
            Errno::Child => "No child processes.",
            Errno::Connaborted => "Connection aborted.",
            Errno::Connrefused => "Connection refused.",
            Errno::Connreset => "Connection reset.",
            Errno::Deadlk => "Resource deadlock would occur.",
            Errno::Destaddrreq => "Destination address required.",
            Errno::Dom => "Mathematics argument out of domain of function.",
            Errno::Dquot => "Reserved.",
            Errno::Exist => "File exists.",
            Errno::Fault => "Bad address.",
            Errno::Fbig => "File too large.",
            Errno::Hostunreach => "Host is unreachable.",
            Errno::Idrm => "Identifier removed.",
            Errno::Ilseq => "Illegal byte sequence.",
            Errno::Inprogress => "Operation in progress.",
            Errno::Intr => "Interrupted function.",
            Errno::Inval => "Invalid argument.",
            Errno::Io => "I/O error.",
            Errno::Isconn => "Socket is connected.",
            Errno::Isdir => "Is a directory.",
            Errno::Loop => "Too many levels of symbolic links.",
            Errno::Mfile => "File descriptor value too large.",
            Errno::Mlink => "Too many links.",
            Errno::Msgsize => "Message too large.",
            Errno::Multihop => "Reserved.",
            Errno::Nametoolong => "Filename too long.",
            Errno::Netdown => "Network is down.",
            Errno::Netreset => "Connection aborted by network.",
            Errno::Netunreach => "Network unreachable.",
            Errno::Nfile => "Too many files open in system.",
            Errno::Nobufs => "No buffer space available.",
            Errno::Nodev => "No such device.",
            Errno::Noent => "No such file or directory.",
            Errno::Noexec => "Executable file format error.",
            Errno::Nolck => "No locks available.",
            Errno::Nolink => "Reserved.",
            Errno::Nomem => "Not enough space.",
            Errno::Nomsg => "No message of the desired type.",
            Errno::Noprotoopt => "Protocol not available.",
            Errno::Nospc => "No space left on device.",
            Errno::Nosys => "Function not supported.",
            Errno::Notconn => "The socket is not connected.",
            Errno::Notdir => "Not a directory or a symbolic link to a directory.",
            Errno::Notempty => "Directory not empty.",
            Errno::Notrecoverable => "State not recoverable.",
            Errno::Notsock => "Not a socket.",
            Errno::Notsup => "Not supported, or operation not supported on socket.",
            Errno::Notty => "Inappropriate I/O control operation.",
            Errno::Nxio => "No such device or address.",
            Errno::Overflow => "Value too large to be stored in data type.",
            Errno::Ownerdead => "Previous owner died.",
            Errno::Perm => "Operation not permitted.",
            Errno::Pipe => "Broken pipe.",
            Errno::Proto => "Protocol error.",
            Errno::Protonosupport => "Protocol not supported.",
            Errno::Prototype => "Protocol wrong type for socket.",
            Errno::Range => "Result too large.",
            Errno::Rofs => "Read-only file system.",
            Errno::Spipe => "Invalid seek.",
            Errno::Srch => "No such process.",
            Errno::Stale => "Reserved.",
            Errno::Timedout => "Connection timed out.",
            Errno::Txtbsy => "Text file busy.",
            Errno::Xdev => "Cross-device link.",
            Errno::Notcapable => "Extension: Capabilities insufficient.",
            Errno::Shutdown => "Cannot send after socket shutdown.",
            Errno::Memviolation => "Memory access violation.",
            Errno::Unknown => "An unknown error has occured",
        }
    }
}
impl core::fmt::Debug for Errno {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Errno::{}", &self.name())
    }
}
impl core::fmt::Display for Errno {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} (error {})", self.name(), *self as i32)
    }
}
impl std::error::Error for Errno {}
wai_bindgen_rust::bitflags::bitflags! {
    #[doc = " File descriptor rights, determining which actions may be performed."]
    #[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
    pub struct Rights : u64 {
        #[doc = " The right to invoke `fd_datasync`."]
        #[doc = " "]
        #[doc = " If `rights::path_open` is set, includes the right to invoke"]
        #[doc = " `path_open` with `fdflags::dsync`."]
        const FD_DATASYNC = 1 << 0;
        #[doc = " The right to invoke `fd_read` and `sock_recv`."]
        #[doc = " "]
        #[doc = " If `rights::fd_seek` is set, includes the right to invoke `fd_pread`."]
        const FD_READ = 1 << 1;
        #[doc = " The right to invoke `fd_seek`. This flag implies `rights::fd_tell`."]
        const FD_SEEK = 1 << 2;
        #[doc = " The right to invoke `fd_fdstat_set_flags`."]
        const FD_FDSTAT_SET_FLAGS = 1 << 3;
        #[doc = " The right to invoke `fd_sync`."]
        #[doc = " "]
        #[doc = " If `rights::path_open` is set, includes the right to invoke"]
        #[doc = " `path_open` with `fdflags::rsync` and `fdflags::dsync`."]
        const FD_SYNC = 1 << 4;
        #[doc = " The right to invoke `fd_seek` in such a way that the file offset"]
        #[doc = " remains unaltered (i.e., `whence::cur` with offset zero), or to"]
        #[doc = " invoke `fd_tell`."]
        const FD_TELL = 1 << 5;
        #[doc = " The right to invoke `fd_write` and `sock_send`."]
        #[doc = " If `rights::fd_seek` is set, includes the right to invoke `fd_pwrite`."]
        const FD_WRITE = 1 << 6;
        #[doc = " The right to invoke `fd_advise`."]
        const FD_ADVISE = 1 << 7;
        #[doc = " The right to invoke `fd_allocate`."]
        const FD_ALLOCATE = 1 << 8;
        #[doc = " The right to invoke `path_create_directory`."]
        const PATH_CREATE_DIRECTORY = 1 << 9;
        #[doc = " If `rights::path_open` is set, the right to invoke `path_open` with `oflags::creat`."]
        const PATH_CREATE_FILE = 1 << 10;
        #[doc = " The right to invoke `path_link` with the file descriptor as the"]
        #[doc = " source directory."]
        const PATH_LINK_SOURCE = 1 << 11;
        #[doc = " The right to invoke `path_link` with the file descriptor as the"]
        #[doc = " target directory."]
        const PATH_LINK_TARGET = 1 << 12;
        #[doc = " The right to invoke `path_open`."]
        const PATH_OPEN = 1 << 13;
        #[doc = " The right to invoke `fd_readdir`."]
        const FD_READDIR = 1 << 14;
        #[doc = " The right to invoke `path_readlink`."]
        const PATH_READLINK = 1 << 15;
        #[doc = " The right to invoke `path_rename` with the file descriptor as the source directory."]
        const PATH_RENAME_SOURCE = 1 << 16;
        #[doc = " The right to invoke `path_rename` with the file descriptor as the target directory."]
        const PATH_RENAME_TARGET = 1 << 17;
        #[doc = " The right to invoke `path_filestat_get`."]
        const PATH_FILESTAT_GET = 1 << 18;
        #[doc = " The right to change a file's size (there is no `path_filestat_set_size`)."]
        #[doc = " If `rights::path_open` is set, includes the right to invoke `path_open` with `oflags::trunc`."]
        const PATH_FILESTAT_SET_SIZE = 1 << 19;
        #[doc = " The right to invoke `path_filestat_set_times`."]
        const PATH_FILESTAT_SET_TIMES = 1 << 20;
        #[doc = " The right to invoke `fd_filestat_get`."]
        const FD_FILESTAT_GET = 1 << 21;
        #[doc = " The right to invoke `fd_filestat_set_size`."]
        const FD_FILESTAT_SET_SIZE = 1 << 22;
        #[doc = " The right to invoke `fd_filestat_set_times`."]
        const FD_FILESTAT_SET_TIMES = 1 << 23;
        #[doc = " The right to invoke `path_symlink`."]
        const PATH_SYMLINK = 1 << 24;
        #[doc = " The right to invoke `path_remove_directory`."]
        const PATH_REMOVE_DIRECTORY = 1 << 25;
        #[doc = " The right to invoke `path_unlink_file`."]
        const PATH_UNLINK_FILE = 1 << 26;
        #[doc = " If `rights::fd_read` is set, includes the right to invoke `poll_oneoff` to subscribe to `eventtype::fd_read`."]
        #[doc = " If `rights::fd_write` is set, includes the right to invoke `poll_oneoff` to subscribe to `eventtype::fd_write`."]
        const POLL_FD_READWRITE = 1 << 27;
        #[doc = " The right to invoke `sock_shutdown`."]
        const SOCK_SHUTDOWN = 1 << 28;
        #[doc = " TODO: Found in wasmer-wasi-types rust project, but not in wasi-snapshot0"]
        const SOCK_ACCEPT = 1 << 29;
        #[doc = " TODO: Found in wasmer-wasi-types rust project, but not in wasi-snapshot0"]
        const SOCK_CONNECT = 1 << 30;
        #[doc = " TODO: Found in wasmer-wasi-types rust project, but not in wasi-snapshot0"]
        const SOCK_LISTEN = 1 << 31;
        #[doc = " TODO: Found in wasmer-wasi-types rust project, but not in wasi-snapshot0"]
        const SOCK_BIND = 1 << 32;
        #[doc = " TODO: Found in wasmer-wasi-types rust project, but not in wasi-snapshot0"]
        const SOCK_RECV = 1 << 33;
        #[doc = " TODO: Found in wasmer-wasi-types rust project, but not in wasi-snapshot0"]
        const SOCK_SEND = 1 << 34;
        #[doc = " TODO: Found in wasmer-wasi-types rust project, but not in wasi-snapshot0"]
        const SOCK_ADDR_LOCAL = 1 << 35;
        #[doc = " TODO: Found in wasmer-wasi-types rust project, but not in wasi-snapshot0"]
        const SOCK_ADDR_REMOTE = 1 << 36;
        # [doc = " TODO: Found in wasmer-wasi-types rust project, but not in wasi-snapshot0"]
        const SOCK_RECV_FROM = 1 << 37;
        #[doc = " TODO: Found in wasmer-wasi-types rust project, but not in wasi-snapshot0"]
        const SOCK_SEND_TO = 1 << 38;
    }
}
impl Rights {
    #[doc = " Convert from a raw integer, preserving any unknown bits. See"]
    #[doc = " <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>"]
    pub fn from_bits_preserve(bits: u64) -> Self {
        Self { bits }
    }
}
#[doc = " The type of a file descriptor or file."]
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Filetype {
    #[doc = " The type of the file descriptor or file is unknown or is different from any of the other types specified."]
    Unknown,
    #[doc = " The file descriptor or file refers to a block device inode."]
    BlockDevice,
    #[doc = " The file descriptor or file refers to a character device inode."]
    CharacterDevice,
    #[doc = " The file descriptor or file refers to a directory inode."]
    Directory,
    #[doc = " The file descriptor or file refers to a regular file inode."]
    RegularFile,
    #[doc = " The file descriptor or file refers to a datagram socket."]
    SocketDgram,
    #[doc = " The file descriptor or file refers to a byte-stream socket."]
    SocketStream,
    #[doc = " The file refers to a symbolic link inode."]
    SymbolicLink,
    #[doc = " The file descriptor or file refers to a raw socket."]
    SocketRaw,
    #[doc = " The file descriptor or file refers to a sequential packet socket."]
    SocketSeqpacket,
}
impl core::fmt::Debug for Filetype {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Filetype::Unknown => f.debug_tuple("Filetype::Unknown").finish(),
            Filetype::BlockDevice => f.debug_tuple("Filetype::BlockDevice").finish(),
            Filetype::CharacterDevice => f.debug_tuple("Filetype::CharacterDevice").finish(),
            Filetype::Directory => f.debug_tuple("Filetype::Directory").finish(),
            Filetype::RegularFile => f.debug_tuple("Filetype::RegularFile").finish(),
            Filetype::SocketDgram => f.debug_tuple("Filetype::SocketDgram").finish(),
            Filetype::SocketStream => f.debug_tuple("Filetype::SocketStream").finish(),
            Filetype::SymbolicLink => f.debug_tuple("Filetype::SymbolicLink").finish(),
            Filetype::SocketRaw => f.debug_tuple("Filetype::SocketRaw").finish(),
            Filetype::SocketSeqpacket => f.debug_tuple("Filetype::SocketSeqpacket").finish(),
        }
    }
}
#[doc = " A directory entry, snapshot0 version."]
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Snapshot0Dirent {
    #[doc = " The offset of the next directory entry stored in this directory."]
    pub d_next: Dircookie,
    #[doc = " The serial number of the file referred to by this directory entry."]
    pub d_ino: Inode,
    #[doc = " The length of the name of the directory entry."]
    pub d_namlen: Dirnamlen,
    #[doc = " The type of the file referred to by this directory entry."]
    pub d_type: Filetype,
}
impl core::fmt::Debug for Snapshot0Dirent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Snapshot0Dirent")
            .field("d-next", &self.d_next)
            .field("d-ino", &self.d_ino)
            .field("d-namlen", &self.d_namlen)
            .field("d-type", &self.d_type)
            .finish()
    }
}
#[doc = " A directory entry."]
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Dirent {
    #[doc = " The offset of the next directory entry stored in this directory."]
    pub d_next: Dircookie,
    #[doc = " The serial number of the file referred to by this directory entry."]
    pub d_ino: Inode,
    #[doc = " The type of the file referred to by this directory entry."]
    pub d_type: Filetype,
    #[doc = " The length of the name of the directory entry."]
    pub d_namlen: Dirnamlen,
}
impl core::fmt::Debug for Dirent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Dirent")
            .field("d-next", &self.d_next)
            .field("d-ino", &self.d_ino)
            .field("d-type", &self.d_type)
            .field("d-namlen", &self.d_namlen)
            .finish()
    }
}
#[doc = " File or memory access pattern advisory information."]
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub enum Advice {
    #[doc = " The application has no advice to give on its behavior with respect to the specified data."]
    Normal,
    #[doc = " The application expects to access the specified data sequentially from lower offsets to higher offsets."]
    Sequential,
    #[doc = " The application expects to access the specified data in a random order."]
    Random,
    #[doc = " The application expects to access the specified data in the near future."]
    Willneed,
    #[doc = " The application expects that it will not access the specified data in the near future."]
    Dontneed,
    #[doc = " The application expects to access the specified data once and then not reuse it thereafter."]
    Noreuse,
    #[doc = " The application expectations are unknown."]
    Unknown = 255,
}
impl core::fmt::Debug for Advice {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Advice::Normal => f.debug_tuple("Advice::Normal").finish(),
            Advice::Sequential => f.debug_tuple("Advice::Sequential").finish(),
            Advice::Random => f.debug_tuple("Advice::Random").finish(),
            Advice::Willneed => f.debug_tuple("Advice::Willneed").finish(),
            Advice::Dontneed => f.debug_tuple("Advice::Dontneed").finish(),
            Advice::Noreuse => f.debug_tuple("Advice::Noreuse").finish(),
            Advice::Unknown => f.debug_tuple("Advice::Unknown").finish(),
        }
    }
}
wai_bindgen_rust::bitflags::bitflags! {
    // Actual file descriptor flags. Note, WASI's fdflags actually represent
    // file table flags, not fd flags... Hence the weird name.
    #[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
    pub struct Fdflagsext : u16 {
        #[doc = " Close this file in the child process when spawning one."]
        const CLOEXEC = 1 << 0;
    }
}
impl Fdflagsext {
    #[doc = " Convert from a raw integer, preserving any unknown bits. See"]
    #[doc = " <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>"]
    pub fn from_bits_preserve(bits: u16) -> Self {
        Self { bits }
    }
}
wai_bindgen_rust::bitflags::bitflags! {
    #[doc = " File descriptor flags."]
    #[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
    pub struct Fdflags : u16 {
        #[doc = " Append mode: Data written to the file is always appended to the file's end."]
        const APPEND = 1 << 0;
        #[doc = " Write according to synchronized I/O data integrity completion. Only the data stored in the file is synchronized."]
        const DSYNC = 1 << 1;
        #[doc = " Non-blocking mode."]
        const NONBLOCK = 1 << 2;
        #[doc = " Synchronized read I/O operations."]
        const RSYNC = 1 << 3;
        #[doc = " Write according to synchronized I/O file integrity completion. In"]
        #[doc = " addition to synchronizing the data stored in the file, the implementation"]
        #[doc = " may also synchronously update the file's metadata."]
        const SYNC = 1 << 4;
    }
}
impl Fdflags {
    #[doc = " Convert from a raw integer, preserving any unknown bits. See"]
    #[doc = " <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>"]
    pub fn from_bits_preserve(bits: u16) -> Self {
        Self { bits }
    }
}
#[doc = " File descriptor attributes."]
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Fdstat {
    #[doc = " File type."]
    pub fs_filetype: Filetype,
    #[doc = " File descriptor flags."]
    pub fs_flags: Fdflags,
    #[doc = " Rights that apply to this file descriptor."]
    pub fs_rights_base: Rights,
    #[doc = " Maximum set of rights that may be installed on new file descriptors that"]
    #[doc = " are created through this file descriptor, e.g., through `path_open`."]
    pub fs_rights_inheriting: Rights,
}
impl core::fmt::Debug for Fdstat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Fdstat")
            .field("fs-filetype", &self.fs_filetype)
            .field("fs-flags", &self.fs_flags)
            .field("fs-rights-base", &self.fs_rights_base)
            .field("fs-rights-inheriting", &self.fs_rights_inheriting)
            .finish()
    }
}
wai_bindgen_rust::bitflags::bitflags! {
    #[doc = " Which file time attributes to adjust."]
    #[doc = " TODO: wit appears to not have support for flags repr"]
    #[doc = " (@witx repr u16)"]
    #[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
    pub struct Fstflags : u16 {
        #[doc = " Adjust the last data access timestamp to the value stored in `filestat::atim`."]
        const SET_ATIM = 1 << 0;
        #[doc = " Adjust the last data access timestamp to the time of clock `clockid::realtime`."]
        const SET_ATIM_NOW = 1 << 1;
        #[doc = " Adjust the last data modification timestamp to the value stored in `filestat::mtim`."]
        const SET_MTIM = 1 << 2;
        #[doc = " Adjust the last data modification timestamp to the time of clock `clockid::realtime`."]
        const SET_MTIM_NOW = 1 << 3;
    }
}
impl Fstflags {
    #[doc = " Convert from a raw integer, preserving any unknown bits. See"]
    #[doc = " <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>"]
    pub fn from_bits_preserve(bits: u16) -> Self {
        Self { bits }
    }
}
wai_bindgen_rust::bitflags::bitflags! {
    #[doc = " Flags determining the method of how paths are resolved."]
    #[doc = " TODO: wit appears to not have support for flags repr"]
    #[doc = " (@witx repr u32)"]
    pub struct Lookup : u32 {
        #[doc = " As long as the resolved path corresponds to a symbolic link, it is expanded."]
        const SYMLINK_FOLLOW = 1 << 0;
    }
}
impl Lookup {
    #[doc = " Convert from a raw integer, preserving any unknown bits. See"]
    #[doc = " <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>"]
    pub fn from_bits_preserve(bits: u32) -> Self {
        Self { bits }
    }
}
wai_bindgen_rust::bitflags::bitflags! {
    #[doc = " Open flags used by `path_open`."]
    #[doc = " TODO: wit appears to not have support for flags repr"]
    #[doc = " (@witx repr u16)"]
    #[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
    pub struct Oflags : u16 {
        #[doc = " Create file if it does not exist."]
        const CREATE = 1 << 0;
        #[doc = " Fail if not a directory."]
        const DIRECTORY = 1 << 1;
        #[doc = " Fail if file already exists."]
        const EXCL = 1 << 2;
        #[doc = " Truncate file to size 0."]
        const TRUNC = 1 << 3;
    }
}
impl Oflags {
    #[doc = " Convert from a raw integer, preserving any unknown bits. See"]
    #[doc = " <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>"]
    pub fn from_bits_preserve(bits: u16) -> Self {
        Self { bits }
    }
}
#[doc = " User-provided value that may be attached to objects that is retained when"]
#[doc = " extracted from the implementation."]
pub type Userdata = u64;
#[doc = " Type of a subscription to an event or its occurrence."]
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub enum Eventtype {
    #[doc = " The time value of clock `subscription_clock::id` has"]
    #[doc = " reached timestamp `subscription_clock::timeout`."]
    Clock,
    #[doc = " File descriptor `subscription_fd_readwrite::fd` has data"]
    #[doc = " available for reading. This event always triggers for regular files."]
    FdRead,
    #[doc = " File descriptor `subscription_fd_readwrite::fd` has capacity"]
    #[doc = " available for writing. This event always triggers for regular files."]
    FdWrite,
    #[doc = " Event type is unknown"]
    Unknown = 255,
}
impl core::fmt::Debug for Eventtype {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Eventtype::Clock => f.debug_tuple("Eventtype::Clock").finish(),
            Eventtype::FdRead => f.debug_tuple("Eventtype::FdRead").finish(),
            Eventtype::FdWrite => f.debug_tuple("Eventtype::FdWrite").finish(),
            Eventtype::Unknown => f.debug_tuple("Eventtype::Unknown").finish(),
        }
    }
}
wai_bindgen_rust::bitflags::bitflags! {
    #[doc = " Flags determining how to interpret the timestamp provided in"]
    #[doc = " `subscription-clock::timeout`."]
    pub struct Subclockflags : u16 {
        #[doc = " If set, treat the timestamp provided in"]
        #[doc = " `subscription-clock::timeout` as an absolute timestamp of clock"]
        #[doc = " `subscription-clock::id`. If clear, treat the timestamp"]
        #[doc = " provided in `subscription-clock::timeout` relative to the"]
        #[doc = " current time value of clock `subscription-clock::id`."]
        const SUBSCRIPTION_CLOCK_ABSTIME = 1 << 0;
    }
}
impl Subclockflags {
    #[doc = " Convert from a raw integer, preserving any unknown bits. See"]
    #[doc = " <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>"]
    pub fn from_bits_preserve(bits: u16) -> Self {
        Self { bits }
    }
}
#[doc = " The contents of a `subscription` when type is `eventtype::clock`."]
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Snapshot0SubscriptionClock {
    #[doc = " The user-defined unique identifier of the clock."]
    pub identifier: Userdata,
    #[doc = " The clock against which to compare the timestamp."]
    pub id: Snapshot0Clockid,
    #[doc = " The absolute or relative timestamp."]
    pub timeout: Timestamp,
    #[doc = " The amount of time that the implementation may wait additionally"]
    #[doc = " to coalesce with other events."]
    pub precision: Timestamp,
    #[doc = " Flags specifying whether the timeout is absolute or relative"]
    pub flags: Subclockflags,
}
impl core::fmt::Debug for Snapshot0SubscriptionClock {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Snapshot0SubscriptionClock")
            .field("identifier", &self.identifier)
            .field("id", &self.id)
            .field("timeout", &self.timeout)
            .field("precision", &self.precision)
            .field("flags", &self.flags)
            .finish()
    }
}
#[doc = " The contents of a `subscription` when type is `eventtype::clock`."]
#[repr(C)]
#[derive(Copy, Clone)]
pub struct SubscriptionClock {
    #[doc = " The clock against which to compare the timestamp."]
    pub clock_id: Clockid,
    #[doc = " The absolute or relative timestamp."]
    pub timeout: Timestamp,
    #[doc = " The amount of time that the implementation may wait additionally"]
    #[doc = " to coalesce with other events."]
    pub precision: Timestamp,
    #[doc = " Flags specifying whether the timeout is absolute or relative"]
    pub flags: Subclockflags,
}
impl core::fmt::Debug for SubscriptionClock {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SubscriptionClock")
            .field("clock-id", &self.clock_id)
            .field("timeout", &self.timeout)
            .field("precision", &self.precision)
            .field("flags", &self.flags)
            .finish()
    }
}
#[doc = " Identifiers for preopened capabilities."]
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Preopentype {
    #[doc = " A pre-opened directory."]
    Dir,
    #[doc = " Unknown."]
    Unknown = 255,
}
impl core::fmt::Debug for Preopentype {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Preopentype::Dir => f.debug_tuple("Preopentype::Dir").finish(),
            Preopentype::Unknown => f.debug_tuple("Preopentype::Unknown").finish(),
        }
    }
}
wai_bindgen_rust::bitflags::bitflags! {
    #[doc = " The state of the file descriptor subscribed to with"]
    #[doc = " `eventtype::fd_read` or `eventtype::fd_write`."]
    #[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
    pub struct Eventrwflags : u16 {
        #[doc = " The peer of this socket has closed or disconnected."]
        const FD_READWRITE_HANGUP = 1 << 0;
    }
}
impl Eventrwflags {
    #[doc = " Convert from a raw integer, preserving any unknown bits. See"]
    #[doc = " <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>"]
    pub fn from_bits_preserve(bits: u16) -> Self {
        Self { bits }
    }
}
#[doc = " The contents of an `event` for the `eventtype::fd_read` and"]
#[doc = " `eventtype::fd_write` variants"]
#[repr(C)]
#[derive(Copy, Clone)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct EventFdReadwrite {
    #[doc = " The number of bytes available for reading or writing."]
    pub nbytes: Filesize,
    #[doc = " The state of the file descriptor."]
    pub flags: Eventrwflags,
}
impl core::fmt::Debug for EventFdReadwrite {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EventFdReadwrite")
            .field("nbytes", &self.nbytes)
            .field("flags", &self.flags)
            .finish()
    }
}
#[doc = " An event that occurred."]
#[doc = " The contents of an `event`."]
#[doc = " An event that occurred."]
#[doc = " The contents of a `subscription`, snapshot0 version."]
#[doc = " The contents of a `subscription`."]
#[doc = " The contents of a `subscription` when the variant is"]
#[doc = " `eventtype::fd_read` or `eventtype::fd_write`."]
#[repr(C)]
#[derive(Copy, Clone)]
pub struct SubscriptionFsReadwrite {
    #[doc = " The file descriptor on which to wait for it to become ready for reading or writing."]
    pub file_descriptor: Fd,
}
impl core::fmt::Debug for SubscriptionFsReadwrite {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SubscriptionFsReadwrite")
            .field("file-descriptor", &self.file_descriptor)
            .finish()
    }
}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub enum Socktype {
    Unknown,
    Stream,
    Dgram,
    Raw,
    Seqpacket,
}
impl core::fmt::Debug for Socktype {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Socktype::Unknown => f.debug_tuple("Socktype::Uknown").finish(),
            Socktype::Stream => f.debug_tuple("Socktype::Stream").finish(),
            Socktype::Dgram => f.debug_tuple("Socktype::Dgram").finish(),
            Socktype::Raw => f.debug_tuple("Socktype::Raw").finish(),
            Socktype::Seqpacket => f.debug_tuple("Socktype::Seqpacket").finish(),
        }
    }
}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Sockstatus {
    Opening,
    Opened,
    Closed,
    Failed,
    Unknown = 255,
}
impl core::fmt::Debug for Sockstatus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Sockstatus::Opening => f.debug_tuple("Sockstatus::Opening").finish(),
            Sockstatus::Opened => f.debug_tuple("Sockstatus::Opened").finish(),
            Sockstatus::Closed => f.debug_tuple("Sockstatus::Closed").finish(),
            Sockstatus::Failed => f.debug_tuple("Sockstatus::Failed").finish(),
            Sockstatus::Unknown => f.debug_tuple("Sockstatus::Unknown").finish(),
        }
    }
}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub enum Sockoption {
    Noop,
    ReusePort,
    ReuseAddr,
    NoDelay,
    DontRoute,
    OnlyV6,
    Broadcast,
    MulticastLoopV4,
    MulticastLoopV6,
    Promiscuous,
    Listening,
    LastError,
    KeepAlive,
    Linger,
    OobInline,
    RecvBufSize,
    SendBufSize,
    RecvLowat,
    SendLowat,
    RecvTimeout,
    SendTimeout,
    ConnectTimeout,
    AcceptTimeout,
    Ttl,
    MulticastTtlV4,
    Type,
    Proto,
}
impl core::fmt::Debug for Sockoption {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Sockoption::Noop => f.debug_tuple("Sockoption::Noop").finish(),
            Sockoption::ReusePort => f.debug_tuple("Sockoption::ReusePort").finish(),
            Sockoption::ReuseAddr => f.debug_tuple("Sockoption::ReuseAddr").finish(),
            Sockoption::NoDelay => f.debug_tuple("Sockoption::NoDelay").finish(),
            Sockoption::DontRoute => f.debug_tuple("Sockoption::DontRoute").finish(),
            Sockoption::OnlyV6 => f.debug_tuple("Sockoption::OnlyV6").finish(),
            Sockoption::Broadcast => f.debug_tuple("Sockoption::Broadcast").finish(),
            Sockoption::MulticastLoopV4 => f.debug_tuple("Sockoption::MulticastLoopV4").finish(),
            Sockoption::MulticastLoopV6 => f.debug_tuple("Sockoption::MulticastLoopV6").finish(),
            Sockoption::Promiscuous => f.debug_tuple("Sockoption::Promiscuous").finish(),
            Sockoption::Listening => f.debug_tuple("Sockoption::Listening").finish(),
            Sockoption::LastError => f.debug_tuple("Sockoption::LastError").finish(),
            Sockoption::KeepAlive => f.debug_tuple("Sockoption::KeepAlive").finish(),
            Sockoption::Linger => f.debug_tuple("Sockoption::Linger").finish(),
            Sockoption::OobInline => f.debug_tuple("Sockoption::OobInline").finish(),
            Sockoption::RecvBufSize => f.debug_tuple("Sockoption::RecvBufSize").finish(),
            Sockoption::SendBufSize => f.debug_tuple("Sockoption::SendBufSize").finish(),
            Sockoption::RecvLowat => f.debug_tuple("Sockoption::RecvLowat").finish(),
            Sockoption::SendLowat => f.debug_tuple("Sockoption::SendLowat").finish(),
            Sockoption::RecvTimeout => f.debug_tuple("Sockoption::RecvTimeout").finish(),
            Sockoption::SendTimeout => f.debug_tuple("Sockoption::SendTimeout").finish(),
            Sockoption::ConnectTimeout => f.debug_tuple("Sockoption::ConnectTimeout").finish(),
            Sockoption::AcceptTimeout => f.debug_tuple("Sockoption::AcceptTimeout").finish(),
            Sockoption::Ttl => f.debug_tuple("Sockoption::Ttl").finish(),
            Sockoption::MulticastTtlV4 => f.debug_tuple("Sockoption::MulticastTtlV4").finish(),
            Sockoption::Type => f.debug_tuple("Sockoption::Type").finish(),
            Sockoption::Proto => f.debug_tuple("Sockoption::Proto").finish(),
        }
    }
}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Streamsecurity {
    Unencrypted,
    AnyEncryption,
    ClassicEncryption,
    DoubleEncryption,
    Unknown = 255,
}
impl core::fmt::Debug for Streamsecurity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Streamsecurity::Unencrypted => f.debug_tuple("Streamsecurity::Unencrypted").finish(),
            Streamsecurity::AnyEncryption => {
                f.debug_tuple("Streamsecurity::AnyEncryption").finish()
            }
            Streamsecurity::ClassicEncryption => {
                f.debug_tuple("Streamsecurity::ClassicEncryption").finish()
            }
            Streamsecurity::DoubleEncryption => {
                f.debug_tuple("Streamsecurity::DoubleEncryption").finish()
            }
            Streamsecurity::Unknown => f.debug_tuple("Streamsecurity::Unknown").finish(),
        }
    }
}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub enum Addressfamily {
    Unspec,
    Inet4,
    Inet6,
    Unix,
}
impl core::fmt::Debug for Addressfamily {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Addressfamily::Unspec => f.debug_tuple("Addressfamily::Unspec").finish(),
            Addressfamily::Inet4 => f.debug_tuple("Addressfamily::Inet4").finish(),
            Addressfamily::Inet6 => f.debug_tuple("Addressfamily::Inet6").finish(),
            Addressfamily::Unix => f.debug_tuple("Addressfamily::Unix").finish(),
        }
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Snapshot0Filestat {
    pub st_dev: Device,
    pub st_ino: Inode,
    pub st_filetype: Filetype,
    pub st_nlink: Snapshot0Linkcount,
    pub st_size: Filesize,
    pub st_atim: Timestamp,
    pub st_mtim: Timestamp,
    pub st_ctim: Timestamp,
}
impl core::fmt::Debug for Snapshot0Filestat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Snapshot0Filestat")
            .field("st-dev", &self.st_dev)
            .field("st-ino", &self.st_ino)
            .field("st-filetype", &self.st_filetype)
            .field("st-nlink", &self.st_nlink)
            .field("st-size", &self.st_size)
            .field("st-atim", &self.st_atim)
            .field("st-mtim", &self.st_mtim)
            .field("st-ctim", &self.st_ctim)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Filestat {
    pub st_dev: Device,
    pub st_ino: Inode,
    pub st_filetype: Filetype,
    pub st_nlink: Linkcount,
    pub st_size: Filesize,
    pub st_atim: Timestamp,
    pub st_mtim: Timestamp,
    pub st_ctim: Timestamp,
}
impl core::fmt::Debug for Filestat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Filestat")
            .field("st-dev", &self.st_dev)
            .field("st-ino", &self.st_ino)
            .field("st-filetype", &self.st_filetype)
            .field("st-nlink", &self.st_nlink)
            .field("st-size", &self.st_size)
            .field("st-atim", &self.st_atim)
            .field("st-mtim", &self.st_mtim)
            .field("st-ctim", &self.st_ctim)
            .finish()
    }
}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Snapshot0Whence {
    Cur,
    End,
    Set,
    Unknown = 255,
}
impl core::fmt::Debug for Snapshot0Whence {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Snapshot0Whence::Cur => f.debug_tuple("Snapshot0Whence::Cur").finish(),
            Snapshot0Whence::End => f.debug_tuple("Snapshot0Whence::End").finish(),
            Snapshot0Whence::Set => f.debug_tuple("Snapshot0Whence::Set").finish(),
            Snapshot0Whence::Unknown => f.debug_tuple("Snapshot0Whence::Unknown").finish(),
        }
    }
}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub enum Whence {
    Set,
    Cur,
    End,
    Unknown = 255,
}
impl core::fmt::Debug for Whence {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Whence::Set => f.debug_tuple("Whence::Set").finish(),
            Whence::Cur => f.debug_tuple("Whence::Cur").finish(),
            Whence::End => f.debug_tuple("Whence::End").finish(),
            Whence::Unknown => f.debug_tuple("Whence::Unknown").finish(),
        }
    }
}
#[repr(C)]
#[derive(Copy, Clone, PartialEq)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct Tty {
    pub cols: u32,
    pub rows: u32,
    pub width: u32,
    pub height: u32,
    pub stdin_tty: bool,
    pub stdout_tty: bool,
    pub stderr_tty: bool,
    pub echo: bool,
    pub line_buffered: bool,
}
impl core::fmt::Debug for Tty {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Tty")
            .field("cols", &self.cols)
            .field("rows", &self.rows)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("stdin-tty", &self.stdin_tty)
            .field("stdout-tty", &self.stdout_tty)
            .field("stderr-tty", &self.stderr_tty)
            .field("echo", &self.echo)
            .field("line-buffered", &self.line_buffered)
            .finish()
    }
}

#[doc = " __wasi_option_t"]
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OptionTag {
    None,
    Some,
}
impl core::fmt::Debug for OptionTag {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            OptionTag::None => f.debug_tuple("OptionTag::None").finish(),
            OptionTag::Some => f.debug_tuple("OptionTag::Some").finish(),
        }
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct OptionPid {
    pub tag: OptionTag,
    pub pid: Pid,
}
impl core::fmt::Debug for OptionPid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OptionPid")
            .field("tag", &self.tag)
            .field("pid", &self.pid)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct OptionFd {
    pub tag: OptionTag,
    pub fd: Fd,
}
impl core::fmt::Debug for OptionFd {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OptionFd")
            .field("tag", &self.tag)
            .field("fd", &self.fd)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct ProcessHandles {
    pub pid: Pid,
    pub stdin: OptionFd,
    pub stdout: OptionFd,
    pub stderr: OptionFd,
}
impl core::fmt::Debug for ProcessHandles {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ProcessHandles")
            .field("pid", &self.pid)
            .field("stdin", &self.stdin)
            .field("stdout", &self.stdout)
            .field("stderr", &self.stderr)
            .finish()
    }
}
pub type EventFdFlags = u16;
#[repr(C)]
#[derive(Copy, Clone)]
pub struct PrestatUDir {
    pub pr_name_len: u32,
}
impl core::fmt::Debug for PrestatUDir {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PrestatUDir")
            .field("pr-name-len", &self.pr_name_len)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct PrestatU {
    pub dir: PrestatUDir,
}
impl core::fmt::Debug for PrestatU {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PrestatU").field("dir", &self.dir).finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Prestat {
    pub pr_type: Preopentype,
    pub u: PrestatU,
}
impl core::fmt::Debug for Prestat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Prestat")
            .field("pr-type", &self.pr_type)
            .field("u", &self.u)
            .finish()
    }
}
pub type FileDelta = i64;
pub type LookupFlags = u32;
pub type Count = u32;
#[repr(C)]
#[derive(Copy, Clone)]
pub struct PipeHandles {
    pub pipe: Fd,
    pub other: Fd,
}
impl core::fmt::Debug for PipeHandles {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PipeHandles")
            .field("pipe", &self.pipe)
            .field("other", &self.other)
            .finish()
    }
}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StdioMode {
    Piped,
    Inherit,
    Null,
    Log,
}
impl core::fmt::Debug for StdioMode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            StdioMode::Piped => f.debug_tuple("StdioMode::Piped").finish(),
            StdioMode::Inherit => f.debug_tuple("StdioMode::Inherit").finish(),
            StdioMode::Null => f.debug_tuple("StdioMode::Null").finish(),
            StdioMode::Log => f.debug_tuple("StdioMode::Log").finish(),
        }
    }
}
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub enum SockProto {
    Ip,
    Icmp,
    Igmp,
    ProtoThree,
    Ipip,
    ProtoFive,
    Tcp,
    ProtoSeven,
    Egp,
    ProtoNine,
    ProtoTen,
    ProtoEleven,
    Pup,
    ProtoThirteen,
    ProtoFourteen,
    ProtoFifteen,
    ProtoSixteen,
    Udp,
    ProtoEighteen,
    ProtoNineteen,
    ProtoTwenty,
    ProtoTwentyone,
    Idp,
    ProtoTwentythree,
    ProtoTwentyfour,
    ProtoTwentyfive,
    ProtoTwentysix,
    ProtoTwentyseven,
    ProtoTwentyeight,
    ProtoTp,
    ProtoThirty,
    ProtoThirtyone,
    ProtoThirtytwo,
    Dccp,
    ProtoThirtyfour,
    ProtoThirtyfive,
    ProtoThirtysix,
    ProtoThirtyseven,
    ProtoThirtyeight,
    ProtoThirtynine,
    ProtoFourty,
    Ipv6,
    ProtoFourtytwo,
    Routing,
    Fragment,
    ProtoFourtyfive,
    Rsvp,
    Gre,
    ProtoFourtyeight,
    ProtoFourtynine,
    Esp,
    Ah,
    ProtoFiftytwo,
    ProtoFiftythree,
    ProtoFiftyfour,
    ProtoFiftyfive,
    ProtoFiftysix,
    ProtoFiftyseven,
    Icmpv6,
    None,
    Dstopts,
    ProtoSixtyone,
    ProtoSixtytwo,
    ProtoSixtythree,
    ProtoSixtyfour,
    ProtoSixtyfive,
    ProtoSixtysix,
    ProtoSixtyseven,
    ProtoSixtyeight,
    ProtoSixtynine,
    ProtoSeventy,
    ProtoSeventyone,
    ProtoSeventytwo,
    ProtoSeventythree,
    ProtoSeventyfour,
    ProtoSeventyfive,
    ProtoSeventysix,
    ProtoSeventyseven,
    ProtoSeventyeight,
    ProtoSeventynine,
    ProtoEighty,
    ProtoEightyone,
    ProtoEightytwo,
    ProtoEightythree,
    ProtoEightyfour,
    ProtoEightyfive,
    ProtoEightysix,
    ProtoEightyseven,
    ProtoEightyeight,
    ProtoEightynine,
    ProtoNinety,
    ProtoNinetyone,
    Mtp,
    ProtoNinetythree,
    Beetph,
    ProtoNinetyfive,
    ProtoNinetysix,
    ProtoNineetyseven,
    Encap,
    ProtoNinetynine,
    ProtoOnehundred,
    ProtoOnehundredandone,
    ProtoOnehundredandtwo,
    Pim,
    ProtoOnehundredandfour,
    ProtoOnehundredandfive,
    ProtoOnehundredandsix,
    ProtoOnehundredandseven,
    Comp,
    ProtoOnehundredandnine,
    ProtoOnehundredandten,
    ProtoOnehundredandeleven,
    ProtoOnehundredandtwelve,
    ProtoOnehundredandthirteen,
    ProtoOnehundredandfourteen,
    ProtoOnehundredandfifteen,
    ProtoOnehundredandsixteen,
    ProtoOnehundredandseventeen,
    ProtoOnehundredandeighteen,
    ProtoOnehundredandnineteen,
    ProtoOnehundredandtwenty,
    ProtoOnehundredandtwentyone,
    ProtoOnehundredandtwentytwo,
    ProtoOnehundredandtwentythree,
    ProtoOnehundredandtwentyfour,
    ProtoOnehundredandtwentyfive,
    ProtoOnehundredandtwentysix,
    ProtoOnehundredandtwentyseven,
    ProtoOnehundredandtwentyeight,
    ProtoOnehundredandtwentynine,
    ProtoOnehundredandthirty,
    ProtoOnehundredandthirtyone,
    Sctp,
    ProtoOnehundredandthirtythree,
    ProtoOnehundredandthirtyfour,
    Mh,
    Udplite,
    Mpls,
    ProtoOnehundredandthirtyeight,
    ProtoOnehundredandthirtynine,
    ProtoOnehundredandfourty,
    ProtoOnehundredandfourtyone,
    ProtoOnehundredandfourtytwo,
    Ethernet,
    ProtoOnehundredandfourtyfour,
    ProtoOnehundredandfourtyfive,
    ProtoOnehundredandfourtysix,
    ProtoOnehundredandfourtyseven,
    ProtoOnehundredandfourtyeight,
    ProtoOnehundredandfourtynine,
    ProtoOnehundredandfifty,
    ProtoOnehundredandfiftyone,
    ProtoOnehundredandfiftytwo,
    ProtoOnehundredandfiftythree,
    ProtoOnehundredandfiftyfour,
    ProtoOnehundredandfiftyfive,
    ProtoOnehundredandfiftysix,
    ProtoOnehundredandfiftyseven,
    ProtoOnehundredandfiftyeight,
    ProtoOnehundredandfiftynine,
    ProtoOnehundredandsixty,
    ProtoOnehundredandsixtyone,
    ProtoOnehundredandsixtytwo,
    ProtoOnehundredandsixtythree,
    ProtoOnehundredandsixtyfour,
    ProtoOnehundredandsixtyfive,
    ProtoOnehundredandsixtysix,
    ProtoOnehundredandsixtyseven,
    ProtoOnehundredandsixtyeight,
    ProtoOnehundredandsixtynine,
    ProtoOnehundredandseventy,
    ProtoOnehundredandseventyone,
    ProtoOnehundredandseventytwo,
    ProtoOnehundredandseventythree,
    ProtoOnehundredandseventyfour,
    ProtoOnehundredandseventyfive,
    ProtoOnehundredandseventysix,
    ProtoOnehundredandseventyseven,
    ProtoOnehundredandseventyeight,
    ProtoOnehundredandseventynine,
    ProtoOnehundredandeighty,
    ProtoOnehundredandeightyone,
    ProtoOnehundredandeightytwo,
    ProtoOnehundredandeightythree,
    ProtoOnehundredandeightyfour,
    ProtoOnehundredandeightyfive,
    ProtoOnehundredandeightysix,
    ProtoOnehundredandeightyseven,
    ProtoOnehundredandeightyeight,
    ProtoOnehundredandeightynine,
    ProtoOnehundredandninety,
    ProtoOnehundredandninetyone,
    ProtoOnehundredandninetytwo,
    ProtoOnehundredandninetythree,
    ProtoOnehundredandninetyfour,
    ProtoOnehundredandninetyfive,
    ProtoOnehundredandninetysix,
    ProtoOnehundredandninetyseven,
    ProtoOnehundredandninetyeight,
    ProtoOnehundredandninetynine,
    ProtoTwohundred,
    ProtoTwohundredandone,
    ProtoTwohundredandtwo,
    ProtoTwohundredandthree,
    ProtoTwohundredandfour,
    ProtoTwohundredandfive,
    ProtoTwohundredandsix,
    ProtoTwohundredandseven,
    ProtoTwohundredandeight,
    ProtoTwohundredandnine,
    ProtoTwohundredandten,
    ProtoTwohundredandeleven,
    ProtoTwohundredandtwelve,
    ProtoTwohundredandthirteen,
    ProtoTwohundredandfourteen,
    ProtoTwohundredandfifteen,
    ProtoTwohundredandsixteen,
    ProtoTwohundredandseventeen,
    ProtoTwohundredandeighteen,
    ProtoTwohundredandnineteen,
    ProtoTwohundredandtwenty,
    ProtoTwohundredandtwentyone,
    ProtoTwohundredandtwentytwo,
    ProtoTwohundredandtwentythree,
    ProtoTwohundredandtwentyfour,
    ProtoTwohundredandtwentyfive,
    ProtoTwohundredandtwentysix,
    ProtoTwohundredandtwentyseven,
    ProtoTwohundredandtwentyeight,
    ProtoTwohundredandtwentynine,
    ProtoTwohundredandthirty,
    ProtoTwohundredandthirtyone,
    ProtoTwohundredandthirtytwo,
    ProtoTwohundredandthirtythree,
    ProtoTwohundredandthirtyfour,
    ProtoTwohundredandthirtyfive,
    ProtoTwohundredandthirtysix,
    ProtoTwohundredandthirtyseven,
    ProtoTwohundredandthirtyeight,
    ProtoTwohundredandthirtynine,
    ProtoTwohundredandfourty,
    ProtoTwohundredandfourtyone,
    ProtoTwohundredandfourtytwo,
    ProtoTwohundredandfourtythree,
    ProtoTwohundredandfourtyfour,
    ProtoTwohundredandfourtyfive,
    ProtoTwohundredandfourtysix,
    ProtoTwohundredandfourtyseven,
    ProtoTwohundredandfourtyeight,
    ProtoTwohundredandfourtynine,
    ProtoTwohundredandfifty,
    ProtoTwohundredandfiftyone,
    ProtoTwohundredandfiftytwo,
    ProtoTwohundredandfiftythree,
    ProtoTwohundredandfiftyfour,
    ProtoRaw,
    ProtoTwohundredandfiftysix,
    ProtoTwohundredandfiftyseven,
    ProtoTwohundredandfiftyeight,
    ProtoTwohundredandfiftynine,
    ProtoTwohundredandsixty,
    ProtoTwohundredandsixtyone,
    Mptcp,
    Max,
}
impl core::fmt::Debug for SockProto {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SockProto::Ip => f.debug_tuple("SockProto::Ip").finish(),
            SockProto::Icmp => f.debug_tuple("SockProto::Icmp").finish(),
            SockProto::Igmp => f.debug_tuple("SockProto::Igmp").finish(),
            SockProto::ProtoThree => f.debug_tuple("SockProto::ProtoThree").finish(),
            SockProto::Ipip => f.debug_tuple("SockProto::Ipip").finish(),
            SockProto::ProtoFive => f.debug_tuple("SockProto::ProtoFive").finish(),
            SockProto::Tcp => f.debug_tuple("SockProto::Tcp").finish(),
            SockProto::ProtoSeven => f.debug_tuple("SockProto::ProtoSeven").finish(),
            SockProto::Egp => f.debug_tuple("SockProto::Egp").finish(),
            SockProto::ProtoNine => f.debug_tuple("SockProto::ProtoNine").finish(),
            SockProto::ProtoTen => f.debug_tuple("SockProto::ProtoTen").finish(),
            SockProto::ProtoEleven => f.debug_tuple("SockProto::ProtoEleven").finish(),
            SockProto::Pup => f.debug_tuple("SockProto::Pup").finish(),
            SockProto::ProtoThirteen => f.debug_tuple("SockProto::ProtoThirteen").finish(),
            SockProto::ProtoFourteen => f.debug_tuple("SockProto::ProtoFourteen").finish(),
            SockProto::ProtoFifteen => f.debug_tuple("SockProto::ProtoFifteen").finish(),
            SockProto::ProtoSixteen => f.debug_tuple("SockProto::ProtoSixteen").finish(),
            SockProto::Udp => f.debug_tuple("SockProto::Udp").finish(),
            SockProto::ProtoEighteen => f.debug_tuple("SockProto::ProtoEighteen").finish(),
            SockProto::ProtoNineteen => f.debug_tuple("SockProto::ProtoNineteen").finish(),
            SockProto::ProtoTwenty => f.debug_tuple("SockProto::ProtoTwenty").finish(),
            SockProto::ProtoTwentyone => f.debug_tuple("SockProto::ProtoTwentyone").finish(),
            SockProto::Idp => f.debug_tuple("SockProto::Idp").finish(),
            SockProto::ProtoTwentythree => f.debug_tuple("SockProto::ProtoTwentythree").finish(),
            SockProto::ProtoTwentyfour => f.debug_tuple("SockProto::ProtoTwentyfour").finish(),
            SockProto::ProtoTwentyfive => f.debug_tuple("SockProto::ProtoTwentyfive").finish(),
            SockProto::ProtoTwentysix => f.debug_tuple("SockProto::ProtoTwentysix").finish(),
            SockProto::ProtoTwentyseven => f.debug_tuple("SockProto::ProtoTwentyseven").finish(),
            SockProto::ProtoTwentyeight => f.debug_tuple("SockProto::ProtoTwentyeight").finish(),
            SockProto::ProtoTp => f.debug_tuple("SockProto::ProtoTp").finish(),
            SockProto::ProtoThirty => f.debug_tuple("SockProto::ProtoThirty").finish(),
            SockProto::ProtoThirtyone => f.debug_tuple("SockProto::ProtoThirtyone").finish(),
            SockProto::ProtoThirtytwo => f.debug_tuple("SockProto::ProtoThirtytwo").finish(),
            SockProto::Dccp => f.debug_tuple("SockProto::Dccp").finish(),
            SockProto::ProtoThirtyfour => f.debug_tuple("SockProto::ProtoThirtyfour").finish(),
            SockProto::ProtoThirtyfive => f.debug_tuple("SockProto::ProtoThirtyfive").finish(),
            SockProto::ProtoThirtysix => f.debug_tuple("SockProto::ProtoThirtysix").finish(),
            SockProto::ProtoThirtyseven => f.debug_tuple("SockProto::ProtoThirtyseven").finish(),
            SockProto::ProtoThirtyeight => f.debug_tuple("SockProto::ProtoThirtyeight").finish(),
            SockProto::ProtoThirtynine => f.debug_tuple("SockProto::ProtoThirtynine").finish(),
            SockProto::ProtoFourty => f.debug_tuple("SockProto::ProtoFourty").finish(),
            SockProto::Ipv6 => f.debug_tuple("SockProto::Ipv6").finish(),
            SockProto::ProtoFourtytwo => f.debug_tuple("SockProto::ProtoFourtytwo").finish(),
            SockProto::Routing => f.debug_tuple("SockProto::Routing").finish(),
            SockProto::Fragment => f.debug_tuple("SockProto::Fragment").finish(),
            SockProto::ProtoFourtyfive => f.debug_tuple("SockProto::ProtoFourtyfive").finish(),
            SockProto::Rsvp => f.debug_tuple("SockProto::Rsvp").finish(),
            SockProto::Gre => f.debug_tuple("SockProto::Gre").finish(),
            SockProto::ProtoFourtyeight => f.debug_tuple("SockProto::ProtoFourtyeight").finish(),
            SockProto::ProtoFourtynine => f.debug_tuple("SockProto::ProtoFourtynine").finish(),
            SockProto::Esp => f.debug_tuple("SockProto::Esp").finish(),
            SockProto::Ah => f.debug_tuple("SockProto::Ah").finish(),
            SockProto::ProtoFiftytwo => f.debug_tuple("SockProto::ProtoFiftytwo").finish(),
            SockProto::ProtoFiftythree => f.debug_tuple("SockProto::ProtoFiftythree").finish(),
            SockProto::ProtoFiftyfour => f.debug_tuple("SockProto::ProtoFiftyfour").finish(),
            SockProto::ProtoFiftyfive => f.debug_tuple("SockProto::ProtoFiftyfive").finish(),
            SockProto::ProtoFiftysix => f.debug_tuple("SockProto::ProtoFiftysix").finish(),
            SockProto::ProtoFiftyseven => f.debug_tuple("SockProto::ProtoFiftyseven").finish(),
            SockProto::Icmpv6 => f.debug_tuple("SockProto::Icmpv6").finish(),
            SockProto::None => f.debug_tuple("SockProto::None").finish(),
            SockProto::Dstopts => f.debug_tuple("SockProto::Dstopts").finish(),
            SockProto::ProtoSixtyone => f.debug_tuple("SockProto::ProtoSixtyone").finish(),
            SockProto::ProtoSixtytwo => f.debug_tuple("SockProto::ProtoSixtytwo").finish(),
            SockProto::ProtoSixtythree => f.debug_tuple("SockProto::ProtoSixtythree").finish(),
            SockProto::ProtoSixtyfour => f.debug_tuple("SockProto::ProtoSixtyfour").finish(),
            SockProto::ProtoSixtyfive => f.debug_tuple("SockProto::ProtoSixtyfive").finish(),
            SockProto::ProtoSixtysix => f.debug_tuple("SockProto::ProtoSixtysix").finish(),
            SockProto::ProtoSixtyseven => f.debug_tuple("SockProto::ProtoSixtyseven").finish(),
            SockProto::ProtoSixtyeight => f.debug_tuple("SockProto::ProtoSixtyeight").finish(),
            SockProto::ProtoSixtynine => f.debug_tuple("SockProto::ProtoSixtynine").finish(),
            SockProto::ProtoSeventy => f.debug_tuple("SockProto::ProtoSeventy").finish(),
            SockProto::ProtoSeventyone => f.debug_tuple("SockProto::ProtoSeventyone").finish(),
            SockProto::ProtoSeventytwo => f.debug_tuple("SockProto::ProtoSeventytwo").finish(),
            SockProto::ProtoSeventythree => f.debug_tuple("SockProto::ProtoSeventythree").finish(),
            SockProto::ProtoSeventyfour => f.debug_tuple("SockProto::ProtoSeventyfour").finish(),
            SockProto::ProtoSeventyfive => f.debug_tuple("SockProto::ProtoSeventyfive").finish(),
            SockProto::ProtoSeventysix => f.debug_tuple("SockProto::ProtoSeventysix").finish(),
            SockProto::ProtoSeventyseven => f.debug_tuple("SockProto::ProtoSeventyseven").finish(),
            SockProto::ProtoSeventyeight => f.debug_tuple("SockProto::ProtoSeventyeight").finish(),
            SockProto::ProtoSeventynine => f.debug_tuple("SockProto::ProtoSeventynine").finish(),
            SockProto::ProtoEighty => f.debug_tuple("SockProto::ProtoEighty").finish(),
            SockProto::ProtoEightyone => f.debug_tuple("SockProto::ProtoEightyone").finish(),
            SockProto::ProtoEightytwo => f.debug_tuple("SockProto::ProtoEightytwo").finish(),
            SockProto::ProtoEightythree => f.debug_tuple("SockProto::ProtoEightythree").finish(),
            SockProto::ProtoEightyfour => f.debug_tuple("SockProto::ProtoEightyfour").finish(),
            SockProto::ProtoEightyfive => f.debug_tuple("SockProto::ProtoEightyfive").finish(),
            SockProto::ProtoEightysix => f.debug_tuple("SockProto::ProtoEightysix").finish(),
            SockProto::ProtoEightyseven => f.debug_tuple("SockProto::ProtoEightyseven").finish(),
            SockProto::ProtoEightyeight => f.debug_tuple("SockProto::ProtoEightyeight").finish(),
            SockProto::ProtoEightynine => f.debug_tuple("SockProto::ProtoEightynine").finish(),
            SockProto::ProtoNinety => f.debug_tuple("SockProto::ProtoNinety").finish(),
            SockProto::ProtoNinetyone => f.debug_tuple("SockProto::ProtoNinetyone").finish(),
            SockProto::Mtp => f.debug_tuple("SockProto::Mtp").finish(),
            SockProto::ProtoNinetythree => f.debug_tuple("SockProto::ProtoNinetythree").finish(),
            SockProto::Beetph => f.debug_tuple("SockProto::Beetph").finish(),
            SockProto::ProtoNinetyfive => f.debug_tuple("SockProto::ProtoNinetyfive").finish(),
            SockProto::ProtoNinetysix => f.debug_tuple("SockProto::ProtoNinetysix").finish(),
            SockProto::ProtoNineetyseven => f.debug_tuple("SockProto::ProtoNineetyseven").finish(),
            SockProto::Encap => f.debug_tuple("SockProto::Encap").finish(),
            SockProto::ProtoNinetynine => f.debug_tuple("SockProto::ProtoNinetynine").finish(),
            SockProto::ProtoOnehundred => f.debug_tuple("SockProto::ProtoOnehundred").finish(),
            SockProto::ProtoOnehundredandone => {
                f.debug_tuple("SockProto::ProtoOnehundredandone").finish()
            }
            SockProto::ProtoOnehundredandtwo => {
                f.debug_tuple("SockProto::ProtoOnehundredandtwo").finish()
            }
            SockProto::Pim => f.debug_tuple("SockProto::Pim").finish(),
            SockProto::ProtoOnehundredandfour => {
                f.debug_tuple("SockProto::ProtoOnehundredandfour").finish()
            }
            SockProto::ProtoOnehundredandfive => {
                f.debug_tuple("SockProto::ProtoOnehundredandfive").finish()
            }
            SockProto::ProtoOnehundredandsix => {
                f.debug_tuple("SockProto::ProtoOnehundredandsix").finish()
            }
            SockProto::ProtoOnehundredandseven => {
                f.debug_tuple("SockProto::ProtoOnehundredandseven").finish()
            }
            SockProto::Comp => f.debug_tuple("SockProto::Comp").finish(),
            SockProto::ProtoOnehundredandnine => {
                f.debug_tuple("SockProto::ProtoOnehundredandnine").finish()
            }
            SockProto::ProtoOnehundredandten => {
                f.debug_tuple("SockProto::ProtoOnehundredandten").finish()
            }
            SockProto::ProtoOnehundredandeleven => f
                .debug_tuple("SockProto::ProtoOnehundredandeleven")
                .finish(),
            SockProto::ProtoOnehundredandtwelve => f
                .debug_tuple("SockProto::ProtoOnehundredandtwelve")
                .finish(),
            SockProto::ProtoOnehundredandthirteen => f
                .debug_tuple("SockProto::ProtoOnehundredandthirteen")
                .finish(),
            SockProto::ProtoOnehundredandfourteen => f
                .debug_tuple("SockProto::ProtoOnehundredandfourteen")
                .finish(),
            SockProto::ProtoOnehundredandfifteen => f
                .debug_tuple("SockProto::ProtoOnehundredandfifteen")
                .finish(),
            SockProto::ProtoOnehundredandsixteen => f
                .debug_tuple("SockProto::ProtoOnehundredandsixteen")
                .finish(),
            SockProto::ProtoOnehundredandseventeen => f
                .debug_tuple("SockProto::ProtoOnehundredandseventeen")
                .finish(),
            SockProto::ProtoOnehundredandeighteen => f
                .debug_tuple("SockProto::ProtoOnehundredandeighteen")
                .finish(),
            SockProto::ProtoOnehundredandnineteen => f
                .debug_tuple("SockProto::ProtoOnehundredandnineteen")
                .finish(),
            SockProto::ProtoOnehundredandtwenty => f
                .debug_tuple("SockProto::ProtoOnehundredandtwenty")
                .finish(),
            SockProto::ProtoOnehundredandtwentyone => f
                .debug_tuple("SockProto::ProtoOnehundredandtwentyone")
                .finish(),
            SockProto::ProtoOnehundredandtwentytwo => f
                .debug_tuple("SockProto::ProtoOnehundredandtwentytwo")
                .finish(),
            SockProto::ProtoOnehundredandtwentythree => f
                .debug_tuple("SockProto::ProtoOnehundredandtwentythree")
                .finish(),
            SockProto::ProtoOnehundredandtwentyfour => f
                .debug_tuple("SockProto::ProtoOnehundredandtwentyfour")
                .finish(),
            SockProto::ProtoOnehundredandtwentyfive => f
                .debug_tuple("SockProto::ProtoOnehundredandtwentyfive")
                .finish(),
            SockProto::ProtoOnehundredandtwentysix => f
                .debug_tuple("SockProto::ProtoOnehundredandtwentysix")
                .finish(),
            SockProto::ProtoOnehundredandtwentyseven => f
                .debug_tuple("SockProto::ProtoOnehundredandtwentyseven")
                .finish(),
            SockProto::ProtoOnehundredandtwentyeight => f
                .debug_tuple("SockProto::ProtoOnehundredandtwentyeight")
                .finish(),
            SockProto::ProtoOnehundredandtwentynine => f
                .debug_tuple("SockProto::ProtoOnehundredandtwentynine")
                .finish(),
            SockProto::ProtoOnehundredandthirty => f
                .debug_tuple("SockProto::ProtoOnehundredandthirty")
                .finish(),
            SockProto::ProtoOnehundredandthirtyone => f
                .debug_tuple("SockProto::ProtoOnehundredandthirtyone")
                .finish(),
            SockProto::Sctp => f.debug_tuple("SockProto::Sctp").finish(),
            SockProto::ProtoOnehundredandthirtythree => f
                .debug_tuple("SockProto::ProtoOnehundredandthirtythree")
                .finish(),
            SockProto::ProtoOnehundredandthirtyfour => f
                .debug_tuple("SockProto::ProtoOnehundredandthirtyfour")
                .finish(),
            SockProto::Mh => f.debug_tuple("SockProto::Mh").finish(),
            SockProto::Udplite => f.debug_tuple("SockProto::Udplite").finish(),
            SockProto::Mpls => f.debug_tuple("SockProto::Mpls").finish(),
            SockProto::ProtoOnehundredandthirtyeight => f
                .debug_tuple("SockProto::ProtoOnehundredandthirtyeight")
                .finish(),
            SockProto::ProtoOnehundredandthirtynine => f
                .debug_tuple("SockProto::ProtoOnehundredandthirtynine")
                .finish(),
            SockProto::ProtoOnehundredandfourty => f
                .debug_tuple("SockProto::ProtoOnehundredandfourty")
                .finish(),
            SockProto::ProtoOnehundredandfourtyone => f
                .debug_tuple("SockProto::ProtoOnehundredandfourtyone")
                .finish(),
            SockProto::ProtoOnehundredandfourtytwo => f
                .debug_tuple("SockProto::ProtoOnehundredandfourtytwo")
                .finish(),
            SockProto::Ethernet => f.debug_tuple("SockProto::Ethernet").finish(),
            SockProto::ProtoOnehundredandfourtyfour => f
                .debug_tuple("SockProto::ProtoOnehundredandfourtyfour")
                .finish(),
            SockProto::ProtoOnehundredandfourtyfive => f
                .debug_tuple("SockProto::ProtoOnehundredandfourtyfive")
                .finish(),
            SockProto::ProtoOnehundredandfourtysix => f
                .debug_tuple("SockProto::ProtoOnehundredandfourtysix")
                .finish(),
            SockProto::ProtoOnehundredandfourtyseven => f
                .debug_tuple("SockProto::ProtoOnehundredandfourtyseven")
                .finish(),
            SockProto::ProtoOnehundredandfourtyeight => f
                .debug_tuple("SockProto::ProtoOnehundredandfourtyeight")
                .finish(),
            SockProto::ProtoOnehundredandfourtynine => f
                .debug_tuple("SockProto::ProtoOnehundredandfourtynine")
                .finish(),
            SockProto::ProtoOnehundredandfifty => {
                f.debug_tuple("SockProto::ProtoOnehundredandfifty").finish()
            }
            SockProto::ProtoOnehundredandfiftyone => f
                .debug_tuple("SockProto::ProtoOnehundredandfiftyone")
                .finish(),
            SockProto::ProtoOnehundredandfiftytwo => f
                .debug_tuple("SockProto::ProtoOnehundredandfiftytwo")
                .finish(),
            SockProto::ProtoOnehundredandfiftythree => f
                .debug_tuple("SockProto::ProtoOnehundredandfiftythree")
                .finish(),
            SockProto::ProtoOnehundredandfiftyfour => f
                .debug_tuple("SockProto::ProtoOnehundredandfiftyfour")
                .finish(),
            SockProto::ProtoOnehundredandfiftyfive => f
                .debug_tuple("SockProto::ProtoOnehundredandfiftyfive")
                .finish(),
            SockProto::ProtoOnehundredandfiftysix => f
                .debug_tuple("SockProto::ProtoOnehundredandfiftysix")
                .finish(),
            SockProto::ProtoOnehundredandfiftyseven => f
                .debug_tuple("SockProto::ProtoOnehundredandfiftyseven")
                .finish(),
            SockProto::ProtoOnehundredandfiftyeight => f
                .debug_tuple("SockProto::ProtoOnehundredandfiftyeight")
                .finish(),
            SockProto::ProtoOnehundredandfiftynine => f
                .debug_tuple("SockProto::ProtoOnehundredandfiftynine")
                .finish(),
            SockProto::ProtoOnehundredandsixty => {
                f.debug_tuple("SockProto::ProtoOnehundredandsixty").finish()
            }
            SockProto::ProtoOnehundredandsixtyone => f
                .debug_tuple("SockProto::ProtoOnehundredandsixtyone")
                .finish(),
            SockProto::ProtoOnehundredandsixtytwo => f
                .debug_tuple("SockProto::ProtoOnehundredandsixtytwo")
                .finish(),
            SockProto::ProtoOnehundredandsixtythree => f
                .debug_tuple("SockProto::ProtoOnehundredandsixtythree")
                .finish(),
            SockProto::ProtoOnehundredandsixtyfour => f
                .debug_tuple("SockProto::ProtoOnehundredandsixtyfour")
                .finish(),
            SockProto::ProtoOnehundredandsixtyfive => f
                .debug_tuple("SockProto::ProtoOnehundredandsixtyfive")
                .finish(),
            SockProto::ProtoOnehundredandsixtysix => f
                .debug_tuple("SockProto::ProtoOnehundredandsixtysix")
                .finish(),
            SockProto::ProtoOnehundredandsixtyseven => f
                .debug_tuple("SockProto::ProtoOnehundredandsixtyseven")
                .finish(),
            SockProto::ProtoOnehundredandsixtyeight => f
                .debug_tuple("SockProto::ProtoOnehundredandsixtyeight")
                .finish(),
            SockProto::ProtoOnehundredandsixtynine => f
                .debug_tuple("SockProto::ProtoOnehundredandsixtynine")
                .finish(),
            SockProto::ProtoOnehundredandseventy => f
                .debug_tuple("SockProto::ProtoOnehundredandseventy")
                .finish(),
            SockProto::ProtoOnehundredandseventyone => f
                .debug_tuple("SockProto::ProtoOnehundredandseventyone")
                .finish(),
            SockProto::ProtoOnehundredandseventytwo => f
                .debug_tuple("SockProto::ProtoOnehundredandseventytwo")
                .finish(),
            SockProto::ProtoOnehundredandseventythree => f
                .debug_tuple("SockProto::ProtoOnehundredandseventythree")
                .finish(),
            SockProto::ProtoOnehundredandseventyfour => f
                .debug_tuple("SockProto::ProtoOnehundredandseventyfour")
                .finish(),
            SockProto::ProtoOnehundredandseventyfive => f
                .debug_tuple("SockProto::ProtoOnehundredandseventyfive")
                .finish(),
            SockProto::ProtoOnehundredandseventysix => f
                .debug_tuple("SockProto::ProtoOnehundredandseventysix")
                .finish(),
            SockProto::ProtoOnehundredandseventyseven => f
                .debug_tuple("SockProto::ProtoOnehundredandseventyseven")
                .finish(),
            SockProto::ProtoOnehundredandseventyeight => f
                .debug_tuple("SockProto::ProtoOnehundredandseventyeight")
                .finish(),
            SockProto::ProtoOnehundredandseventynine => f
                .debug_tuple("SockProto::ProtoOnehundredandseventynine")
                .finish(),
            SockProto::ProtoOnehundredandeighty => f
                .debug_tuple("SockProto::ProtoOnehundredandeighty")
                .finish(),
            SockProto::ProtoOnehundredandeightyone => f
                .debug_tuple("SockProto::ProtoOnehundredandeightyone")
                .finish(),
            SockProto::ProtoOnehundredandeightytwo => f
                .debug_tuple("SockProto::ProtoOnehundredandeightytwo")
                .finish(),
            SockProto::ProtoOnehundredandeightythree => f
                .debug_tuple("SockProto::ProtoOnehundredandeightythree")
                .finish(),
            SockProto::ProtoOnehundredandeightyfour => f
                .debug_tuple("SockProto::ProtoOnehundredandeightyfour")
                .finish(),
            SockProto::ProtoOnehundredandeightyfive => f
                .debug_tuple("SockProto::ProtoOnehundredandeightyfive")
                .finish(),
            SockProto::ProtoOnehundredandeightysix => f
                .debug_tuple("SockProto::ProtoOnehundredandeightysix")
                .finish(),
            SockProto::ProtoOnehundredandeightyseven => f
                .debug_tuple("SockProto::ProtoOnehundredandeightyseven")
                .finish(),
            SockProto::ProtoOnehundredandeightyeight => f
                .debug_tuple("SockProto::ProtoOnehundredandeightyeight")
                .finish(),
            SockProto::ProtoOnehundredandeightynine => f
                .debug_tuple("SockProto::ProtoOnehundredandeightynine")
                .finish(),
            SockProto::ProtoOnehundredandninety => f
                .debug_tuple("SockProto::ProtoOnehundredandninety")
                .finish(),
            SockProto::ProtoOnehundredandninetyone => f
                .debug_tuple("SockProto::ProtoOnehundredandninetyone")
                .finish(),
            SockProto::ProtoOnehundredandninetytwo => f
                .debug_tuple("SockProto::ProtoOnehundredandninetytwo")
                .finish(),
            SockProto::ProtoOnehundredandninetythree => f
                .debug_tuple("SockProto::ProtoOnehundredandninetythree")
                .finish(),
            SockProto::ProtoOnehundredandninetyfour => f
                .debug_tuple("SockProto::ProtoOnehundredandninetyfour")
                .finish(),
            SockProto::ProtoOnehundredandninetyfive => f
                .debug_tuple("SockProto::ProtoOnehundredandninetyfive")
                .finish(),
            SockProto::ProtoOnehundredandninetysix => f
                .debug_tuple("SockProto::ProtoOnehundredandninetysix")
                .finish(),
            SockProto::ProtoOnehundredandninetyseven => f
                .debug_tuple("SockProto::ProtoOnehundredandninetyseven")
                .finish(),
            SockProto::ProtoOnehundredandninetyeight => f
                .debug_tuple("SockProto::ProtoOnehundredandninetyeight")
                .finish(),
            SockProto::ProtoOnehundredandninetynine => f
                .debug_tuple("SockProto::ProtoOnehundredandninetynine")
                .finish(),
            SockProto::ProtoTwohundred => f.debug_tuple("SockProto::ProtoTwohundred").finish(),
            SockProto::ProtoTwohundredandone => {
                f.debug_tuple("SockProto::ProtoTwohundredandone").finish()
            }
            SockProto::ProtoTwohundredandtwo => {
                f.debug_tuple("SockProto::ProtoTwohundredandtwo").finish()
            }
            SockProto::ProtoTwohundredandthree => {
                f.debug_tuple("SockProto::ProtoTwohundredandthree").finish()
            }
            SockProto::ProtoTwohundredandfour => {
                f.debug_tuple("SockProto::ProtoTwohundredandfour").finish()
            }
            SockProto::ProtoTwohundredandfive => {
                f.debug_tuple("SockProto::ProtoTwohundredandfive").finish()
            }
            SockProto::ProtoTwohundredandsix => {
                f.debug_tuple("SockProto::ProtoTwohundredandsix").finish()
            }
            SockProto::ProtoTwohundredandseven => {
                f.debug_tuple("SockProto::ProtoTwohundredandseven").finish()
            }
            SockProto::ProtoTwohundredandeight => {
                f.debug_tuple("SockProto::ProtoTwohundredandeight").finish()
            }
            SockProto::ProtoTwohundredandnine => {
                f.debug_tuple("SockProto::ProtoTwohundredandnine").finish()
            }
            SockProto::ProtoTwohundredandten => {
                f.debug_tuple("SockProto::ProtoTwohundredandten").finish()
            }
            SockProto::ProtoTwohundredandeleven => f
                .debug_tuple("SockProto::ProtoTwohundredandeleven")
                .finish(),
            SockProto::ProtoTwohundredandtwelve => f
                .debug_tuple("SockProto::ProtoTwohundredandtwelve")
                .finish(),
            SockProto::ProtoTwohundredandthirteen => f
                .debug_tuple("SockProto::ProtoTwohundredandthirteen")
                .finish(),
            SockProto::ProtoTwohundredandfourteen => f
                .debug_tuple("SockProto::ProtoTwohundredandfourteen")
                .finish(),
            SockProto::ProtoTwohundredandfifteen => f
                .debug_tuple("SockProto::ProtoTwohundredandfifteen")
                .finish(),
            SockProto::ProtoTwohundredandsixteen => f
                .debug_tuple("SockProto::ProtoTwohundredandsixteen")
                .finish(),
            SockProto::ProtoTwohundredandseventeen => f
                .debug_tuple("SockProto::ProtoTwohundredandseventeen")
                .finish(),
            SockProto::ProtoTwohundredandeighteen => f
                .debug_tuple("SockProto::ProtoTwohundredandeighteen")
                .finish(),
            SockProto::ProtoTwohundredandnineteen => f
                .debug_tuple("SockProto::ProtoTwohundredandnineteen")
                .finish(),
            SockProto::ProtoTwohundredandtwenty => f
                .debug_tuple("SockProto::ProtoTwohundredandtwenty")
                .finish(),
            SockProto::ProtoTwohundredandtwentyone => f
                .debug_tuple("SockProto::ProtoTwohundredandtwentyone")
                .finish(),
            SockProto::ProtoTwohundredandtwentytwo => f
                .debug_tuple("SockProto::ProtoTwohundredandtwentytwo")
                .finish(),
            SockProto::ProtoTwohundredandtwentythree => f
                .debug_tuple("SockProto::ProtoTwohundredandtwentythree")
                .finish(),
            SockProto::ProtoTwohundredandtwentyfour => f
                .debug_tuple("SockProto::ProtoTwohundredandtwentyfour")
                .finish(),
            SockProto::ProtoTwohundredandtwentyfive => f
                .debug_tuple("SockProto::ProtoTwohundredandtwentyfive")
                .finish(),
            SockProto::ProtoTwohundredandtwentysix => f
                .debug_tuple("SockProto::ProtoTwohundredandtwentysix")
                .finish(),
            SockProto::ProtoTwohundredandtwentyseven => f
                .debug_tuple("SockProto::ProtoTwohundredandtwentyseven")
                .finish(),
            SockProto::ProtoTwohundredandtwentyeight => f
                .debug_tuple("SockProto::ProtoTwohundredandtwentyeight")
                .finish(),
            SockProto::ProtoTwohundredandtwentynine => f
                .debug_tuple("SockProto::ProtoTwohundredandtwentynine")
                .finish(),
            SockProto::ProtoTwohundredandthirty => f
                .debug_tuple("SockProto::ProtoTwohundredandthirty")
                .finish(),
            SockProto::ProtoTwohundredandthirtyone => f
                .debug_tuple("SockProto::ProtoTwohundredandthirtyone")
                .finish(),
            SockProto::ProtoTwohundredandthirtytwo => f
                .debug_tuple("SockProto::ProtoTwohundredandthirtytwo")
                .finish(),
            SockProto::ProtoTwohundredandthirtythree => f
                .debug_tuple("SockProto::ProtoTwohundredandthirtythree")
                .finish(),
            SockProto::ProtoTwohundredandthirtyfour => f
                .debug_tuple("SockProto::ProtoTwohundredandthirtyfour")
                .finish(),
            SockProto::ProtoTwohundredandthirtyfive => f
                .debug_tuple("SockProto::ProtoTwohundredandthirtyfive")
                .finish(),
            SockProto::ProtoTwohundredandthirtysix => f
                .debug_tuple("SockProto::ProtoTwohundredandthirtysix")
                .finish(),
            SockProto::ProtoTwohundredandthirtyseven => f
                .debug_tuple("SockProto::ProtoTwohundredandthirtyseven")
                .finish(),
            SockProto::ProtoTwohundredandthirtyeight => f
                .debug_tuple("SockProto::ProtoTwohundredandthirtyeight")
                .finish(),
            SockProto::ProtoTwohundredandthirtynine => f
                .debug_tuple("SockProto::ProtoTwohundredandthirtynine")
                .finish(),
            SockProto::ProtoTwohundredandfourty => f
                .debug_tuple("SockProto::ProtoTwohundredandfourty")
                .finish(),
            SockProto::ProtoTwohundredandfourtyone => f
                .debug_tuple("SockProto::ProtoTwohundredandfourtyone")
                .finish(),
            SockProto::ProtoTwohundredandfourtytwo => f
                .debug_tuple("SockProto::ProtoTwohundredandfourtytwo")
                .finish(),
            SockProto::ProtoTwohundredandfourtythree => f
                .debug_tuple("SockProto::ProtoTwohundredandfourtythree")
                .finish(),
            SockProto::ProtoTwohundredandfourtyfour => f
                .debug_tuple("SockProto::ProtoTwohundredandfourtyfour")
                .finish(),
            SockProto::ProtoTwohundredandfourtyfive => f
                .debug_tuple("SockProto::ProtoTwohundredandfourtyfive")
                .finish(),
            SockProto::ProtoTwohundredandfourtysix => f
                .debug_tuple("SockProto::ProtoTwohundredandfourtysix")
                .finish(),
            SockProto::ProtoTwohundredandfourtyseven => f
                .debug_tuple("SockProto::ProtoTwohundredandfourtyseven")
                .finish(),
            SockProto::ProtoTwohundredandfourtyeight => f
                .debug_tuple("SockProto::ProtoTwohundredandfourtyeight")
                .finish(),
            SockProto::ProtoTwohundredandfourtynine => f
                .debug_tuple("SockProto::ProtoTwohundredandfourtynine")
                .finish(),
            SockProto::ProtoTwohundredandfifty => {
                f.debug_tuple("SockProto::ProtoTwohundredandfifty").finish()
            }
            SockProto::ProtoTwohundredandfiftyone => f
                .debug_tuple("SockProto::ProtoTwohundredandfiftyone")
                .finish(),
            SockProto::ProtoTwohundredandfiftytwo => f
                .debug_tuple("SockProto::ProtoTwohundredandfiftytwo")
                .finish(),
            SockProto::ProtoTwohundredandfiftythree => f
                .debug_tuple("SockProto::ProtoTwohundredandfiftythree")
                .finish(),
            SockProto::ProtoTwohundredandfiftyfour => f
                .debug_tuple("SockProto::ProtoTwohundredandfiftyfour")
                .finish(),
            SockProto::ProtoRaw => f.debug_tuple("SockProto::ProtoRaw").finish(),
            SockProto::ProtoTwohundredandfiftysix => f
                .debug_tuple("SockProto::ProtoTwohundredandfiftysix")
                .finish(),
            SockProto::ProtoTwohundredandfiftyseven => f
                .debug_tuple("SockProto::ProtoTwohundredandfiftyseven")
                .finish(),
            SockProto::ProtoTwohundredandfiftyeight => f
                .debug_tuple("SockProto::ProtoTwohundredandfiftyeight")
                .finish(),
            SockProto::ProtoTwohundredandfiftynine => f
                .debug_tuple("SockProto::ProtoTwohundredandfiftynine")
                .finish(),
            SockProto::ProtoTwohundredandsixty => {
                f.debug_tuple("SockProto::ProtoTwohundredandsixty").finish()
            }
            SockProto::ProtoTwohundredandsixtyone => f
                .debug_tuple("SockProto::ProtoTwohundredandsixtyone")
                .finish(),
            SockProto::Mptcp => f.debug_tuple("SockProto::Mptcp").finish(),
            SockProto::Max => f.debug_tuple("SockProto::Max").finish(),
        }
    }
}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Bool {
    False,
    True,
}
impl core::fmt::Debug for Bool {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Bool::False => f.debug_tuple("Bool::False").finish(),
            Bool::True => f.debug_tuple("Bool::True").finish(),
        }
    }
}
impl core::fmt::Display for Bool {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Bool::False => write!(f, "false"),
            Bool::True => write!(f, "true"),
        }
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct OptionTimestamp {
    pub tag: OptionTag,
    pub u: Timestamp,
}
impl core::fmt::Debug for OptionTimestamp {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OptionTimestamp")
            .field("tag", &self.tag)
            .field("u", &self.u)
            .finish()
    }
}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, num_enum :: TryFromPrimitive, Hash)]
pub enum Signal {
    Signone = 0,
    Sighup,
    Sigint,
    Sigquit,
    Sigill,
    Sigtrap,
    Sigabrt,
    Sigbus,
    Sigfpe,
    Sigkill,
    Sigusr1,
    Sigsegv,
    Sigusr2,
    Sigpipe,
    Sigalrm,
    Sigterm,
    Sigstkflt,
    Sigchld,
    Sigcont,
    Sigstop,
    Sigtstp,
    Sigttin,
    Sigttou,
    Sigurg,
    Sigxcpu,
    Sigxfsz,
    Sigvtalrm,
    Sigprof,
    Sigwinch,
    Sigpoll,
    Sigpwr,
    Sigsys,
}
impl core::fmt::Debug for Signal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Signal::Signone => f.debug_tuple("Signal::Signone").finish(),
            Signal::Sighup => f.debug_tuple("Signal::Sighup").finish(),
            Signal::Sigint => f.debug_tuple("Signal::Sigint").finish(),
            Signal::Sigquit => f.debug_tuple("Signal::Sigquit").finish(),
            Signal::Sigill => f.debug_tuple("Signal::Sigill").finish(),
            Signal::Sigtrap => f.debug_tuple("Signal::Sigtrap").finish(),
            Signal::Sigabrt => f.debug_tuple("Signal::Sigabrt").finish(),
            Signal::Sigbus => f.debug_tuple("Signal::Sigbus").finish(),
            Signal::Sigfpe => f.debug_tuple("Signal::Sigfpe").finish(),
            Signal::Sigkill => f.debug_tuple("Signal::Sigkill").finish(),
            Signal::Sigusr1 => f.debug_tuple("Signal::Sigusr1").finish(),
            Signal::Sigsegv => f.debug_tuple("Signal::Sigsegv").finish(),
            Signal::Sigusr2 => f.debug_tuple("Signal::Sigusr2").finish(),
            Signal::Sigpipe => f.debug_tuple("Signal::Sigpipe").finish(),
            Signal::Sigalrm => f.debug_tuple("Signal::Sigalrm").finish(),
            Signal::Sigterm => f.debug_tuple("Signal::Sigterm").finish(),
            Signal::Sigstkflt => f.debug_tuple("Signal::Sigstkflt").finish(),
            Signal::Sigchld => f.debug_tuple("Signal::Sigchld").finish(),
            Signal::Sigcont => f.debug_tuple("Signal::Sigcont").finish(),
            Signal::Sigstop => f.debug_tuple("Signal::Sigstop").finish(),
            Signal::Sigtstp => f.debug_tuple("Signal::Sigtstp").finish(),
            Signal::Sigttin => f.debug_tuple("Signal::Sigttin").finish(),
            Signal::Sigttou => f.debug_tuple("Signal::Sigttou").finish(),
            Signal::Sigurg => f.debug_tuple("Signal::Sigurg").finish(),
            Signal::Sigxcpu => f.debug_tuple("Signal::Sigxcpu").finish(),
            Signal::Sigxfsz => f.debug_tuple("Signal::Sigxfsz").finish(),
            Signal::Sigvtalrm => f.debug_tuple("Signal::Sigvtalrm").finish(),
            Signal::Sigprof => f.debug_tuple("Signal::Sigprof").finish(),
            Signal::Sigwinch => f.debug_tuple("Signal::Sigwinch").finish(),
            Signal::Sigpoll => f.debug_tuple("Signal::Sigpoll").finish(),
            Signal::Sigpwr => f.debug_tuple("Signal::Sigpwr").finish(),
            Signal::Sigsys => f.debug_tuple("Signal::Sigsys").finish(),
        }
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct AddrUnspec {
    pub n0: u8,
}
impl core::fmt::Debug for AddrUnspec {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AddrUnspec").field("n0", &self.n0).finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct AddrUnspecPort {
    pub port: u16,
    pub addr: AddrUnspec,
}
impl core::fmt::Debug for AddrUnspecPort {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AddrUnspecPort")
            .field("port", &self.port)
            .field("addr", &self.addr)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct CidrUnspec {
    pub addr: AddrUnspec,
    pub prefix: u8,
}
impl core::fmt::Debug for CidrUnspec {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CidrUnspec")
            .field("addr", &self.addr)
            .field("prefix", &self.prefix)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct HttpHandles {
    pub req: Fd,
    pub res: Fd,
    pub hdr: Fd,
}
impl core::fmt::Debug for HttpHandles {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HttpHandles")
            .field("req", &self.req)
            .field("res", &self.res)
            .field("hdr", &self.hdr)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct HttpStatus {
    pub ok: Bool,
    pub redirect: Bool,
    pub size: Filesize,
    pub status: u16,
}
impl core::fmt::Debug for HttpStatus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HttpStatus")
            .field("ok", &self.ok)
            .field("redirect", &self.redirect)
            .field("size", &self.size)
            .field("status", &self.status)
            .finish()
    }
}
pub type RiFlags = u16;
pub type RoFlags = u16;
pub type SdFlags = u8;
pub type SiFlags = u16;
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Timeout {
    Read,
    Write,
    Connect,
    Accept,
    Unknown = 255,
}
impl core::fmt::Debug for Timeout {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Timeout::Read => f.debug_tuple("Timeout::Read").finish(),
            Timeout::Write => f.debug_tuple("Timeout::Write").finish(),
            Timeout::Connect => f.debug_tuple("Timeout::Connect").finish(),
            Timeout::Accept => f.debug_tuple("Timeout::Accept").finish(),
            Timeout::Unknown => f.debug_tuple("Timeout::Unknown").finish(),
        }
    }
}
wai_bindgen_rust::bitflags::bitflags! {
    #[doc = " join flags."]
    pub struct JoinFlags : u32 {
        #[doc = " Non-blocking join on the process"]
        const NON_BLOCKING = 1 << 0 ;
        #[doc = " Return if a process is stopped"]
        const WAKE_STOPPED = 1 << 1 ;
    }
}
impl JoinFlags {
    #[doc = " Convert from a raw integer, preserving any unknown bits. See"]
    #[doc = " <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>"]
    pub fn from_bits_preserve(bits: u32) -> Self {
        Self { bits }
    }
}
#[doc = " What has happened with the proccess when we joined on it"]
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum JoinStatusType {
    #[doc = " Nothing has happened"]
    Nothing,
    #[doc = " The process has exited by a normal exit code"]
    ExitNormal,
    #[doc = " The process was terminated by a signal"]
    ExitSignal,
    #[doc = " The process was stopped by a signal and can be resumed with SIGCONT"]
    Stopped,
}
impl core::fmt::Debug for JoinStatusType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            JoinStatusType::Nothing => f.debug_tuple("JoinStatusType::Nothing").finish(),
            JoinStatusType::ExitNormal => f.debug_tuple("JoinStatusType::ExitNormal").finish(),
            JoinStatusType::ExitSignal => f.debug_tuple("JoinStatusType::ExitSignal").finish(),
            JoinStatusType::Stopped => f.debug_tuple("JoinStatusType::Stopped").finish(),
        }
    }
}
#[doc = " Represents an errno and a signal"]
#[repr(C)]
#[derive(Copy, Clone)]
pub struct ErrnoSignal {
    #[doc = " The exit code that was returned"]
    pub exit_code: Errno,
    #[doc = " The signal that was returned"]
    pub signal: Signal,
}
impl core::fmt::Debug for ErrnoSignal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ErrnoSignal")
            .field("exit-code", &self.exit_code)
            .field("signal", &self.signal)
            .finish()
    }
}

wai_bindgen_rust::bitflags::bitflags! {
    #[doc = " thread state flags"]
    pub struct ThreadStateFlags : u16 {
        const TSD_USED = 1 << 0 ;
        const DLERROR_FLAG = 1 << 1 ;
    }
}
impl ThreadStateFlags {
    #[doc = " Convert from a raw integer, preserving any unknown bits. See"]
    #[doc = " <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>"]
    pub fn from_bits_preserve(bits: u16) -> Self {
        Self { bits }
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Snapshot0Clockid {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl wasmer::FromToNativeWasmType for Snapshot0Clockid {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }

    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Realtime,
            1 => Self::Monotonic,
            2 => Self::ProcessCputimeId,
            3 => Self::ThreadCputimeId,

            q => {
                tracing::debug!("could not serialize number {q} to enum Snapshot0Clockid");
                Self::Unknown
            }
        }
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Clockid {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl wasmer::FromToNativeWasmType for Clockid {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }

    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Realtime,
            1 => Self::Monotonic,
            2 => Self::ProcessCputimeId,
            3 => Self::ThreadCputimeId,

            q => {
                tracing::debug!("could not serialize number {q} to enum Clockid");
                Self::Unknown
            }
        }
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Errno {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl wasmer::FromToNativeWasmType for Errno {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }

    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Success,
            1 => Self::Toobig,
            2 => Self::Access,
            3 => Self::Addrinuse,
            4 => Self::Addrnotavail,
            5 => Self::Afnosupport,
            6 => Self::Again,
            7 => Self::Already,
            8 => Self::Badf,
            9 => Self::Badmsg,
            10 => Self::Busy,
            11 => Self::Canceled,
            12 => Self::Child,
            13 => Self::Connaborted,
            14 => Self::Connrefused,
            15 => Self::Connreset,
            16 => Self::Deadlk,
            17 => Self::Destaddrreq,
            18 => Self::Dom,
            19 => Self::Dquot,
            20 => Self::Exist,
            21 => Self::Fault,
            22 => Self::Fbig,
            23 => Self::Hostunreach,
            24 => Self::Idrm,
            25 => Self::Ilseq,
            26 => Self::Inprogress,
            27 => Self::Intr,
            28 => Self::Inval,
            29 => Self::Io,
            30 => Self::Isconn,
            31 => Self::Isdir,
            32 => Self::Loop,
            33 => Self::Mfile,
            34 => Self::Mlink,
            35 => Self::Msgsize,
            36 => Self::Multihop,
            37 => Self::Nametoolong,
            38 => Self::Netdown,
            39 => Self::Netreset,
            40 => Self::Netunreach,
            41 => Self::Nfile,
            42 => Self::Nobufs,
            43 => Self::Nodev,
            44 => Self::Noent,
            45 => Self::Noexec,
            46 => Self::Nolck,
            47 => Self::Nolink,
            48 => Self::Nomem,
            49 => Self::Nomsg,
            50 => Self::Noprotoopt,
            51 => Self::Nospc,
            52 => Self::Nosys,
            53 => Self::Notconn,
            54 => Self::Notdir,
            55 => Self::Notempty,
            56 => Self::Notrecoverable,
            57 => Self::Notsock,
            58 => Self::Notsup,
            59 => Self::Notty,
            60 => Self::Nxio,
            61 => Self::Overflow,
            62 => Self::Ownerdead,
            63 => Self::Perm,
            64 => Self::Pipe,
            65 => Self::Proto,
            66 => Self::Protonosupport,
            67 => Self::Prototype,
            68 => Self::Range,
            69 => Self::Rofs,
            70 => Self::Spipe,
            71 => Self::Srch,
            72 => Self::Stale,
            73 => Self::Timedout,
            74 => Self::Txtbsy,
            75 => Self::Xdev,
            76 => Self::Notcapable,
            77 => Self::Shutdown,
            78 => Self::Memviolation,
            79 => Self::Unknown,

            _ => Self::Unknown,
        }
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Rights {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Filetype {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl wasmer::FromToNativeWasmType for Filetype {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }

    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Unknown,
            1 => Self::BlockDevice,
            2 => Self::CharacterDevice,
            3 => Self::Directory,
            4 => Self::RegularFile,
            5 => Self::SocketDgram,
            6 => Self::SocketStream,
            7 => Self::SymbolicLink,
            8 => Self::SocketRaw,
            9 => Self::SocketSeqpacket,

            q => {
                tracing::debug!("could not serialize number {q} to enum Filetype");
                Self::Unknown
            }
        }
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Snapshot0Dirent {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Dirent {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Advice {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl wasmer::FromToNativeWasmType for Advice {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }

    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Normal,
            1 => Self::Sequential,
            2 => Self::Random,
            3 => Self::Willneed,
            4 => Self::Dontneed,
            5 => Self::Noreuse,

            q => {
                tracing::debug!("could not serialize number {q} to enum Advice");
                Self::Unknown
            }
        }
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Fdflagsext {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Fdflags {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Fdstat {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Fstflags {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Lookup {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Oflags {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Eventtype {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl wasmer::FromToNativeWasmType for Eventtype {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }

    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Clock,
            1 => Self::FdRead,
            2 => Self::FdWrite,

            q => {
                tracing::debug!("could not serialize number {q} to enum Eventtype");
                Self::Unknown
            }
        }
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Subclockflags {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Snapshot0SubscriptionClock {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for SubscriptionClock {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Preopentype {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl wasmer::FromToNativeWasmType for Preopentype {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }

    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Dir,

            q => {
                tracing::debug!("could not serialize number {q} to enum Preopentype");
                Self::Unknown
            }
        }
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Eventrwflags {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for EventFdReadwrite {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for SubscriptionFsReadwrite {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Socktype {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl wasmer::FromToNativeWasmType for Socktype {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }

    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Unknown,
            1 => Self::Stream,
            2 => Self::Dgram,
            3 => Self::Raw,
            4 => Self::Seqpacket,

            q => {
                tracing::debug!("could not serialize number {q} to enum Socktype");
                Self::Unknown
            }
        }
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Sockstatus {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl wasmer::FromToNativeWasmType for Sockstatus {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }

    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Opening,
            1 => Self::Opened,
            2 => Self::Closed,
            3 => Self::Failed,

            q => {
                tracing::debug!("could not serialize number {q} to enum Sockstatus");
                Self::Unknown
            }
        }
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Sockoption {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl wasmer::FromToNativeWasmType for Sockoption {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }

    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Noop,
            1 => Self::ReusePort,
            2 => Self::ReuseAddr,
            3 => Self::NoDelay,
            4 => Self::DontRoute,
            5 => Self::OnlyV6,
            6 => Self::Broadcast,
            7 => Self::MulticastLoopV4,
            8 => Self::MulticastLoopV6,
            9 => Self::Promiscuous,
            10 => Self::Listening,
            11 => Self::LastError,
            12 => Self::KeepAlive,
            13 => Self::Linger,
            14 => Self::OobInline,
            15 => Self::RecvBufSize,
            16 => Self::SendBufSize,
            17 => Self::RecvLowat,
            18 => Self::SendLowat,
            19 => Self::RecvTimeout,
            20 => Self::SendTimeout,
            21 => Self::ConnectTimeout,
            22 => Self::AcceptTimeout,
            23 => Self::Ttl,
            24 => Self::MulticastTtlV4,
            25 => Self::Type,
            26 => Self::Proto,

            q => {
                tracing::debug!("could not serialize number {q} to enum Sockoption");
                Self::Noop
            }
        }
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Streamsecurity {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl wasmer::FromToNativeWasmType for Streamsecurity {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }

    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Unencrypted,
            1 => Self::AnyEncryption,
            2 => Self::ClassicEncryption,
            3 => Self::DoubleEncryption,

            q => {
                tracing::debug!("could not serialize number {q} to enum Streamsecurity");
                Self::Unknown
            }
        }
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Addressfamily {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl wasmer::FromToNativeWasmType for Addressfamily {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }

    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Unspec,
            1 => Self::Inet4,
            2 => Self::Inet6,
            3 => Self::Unix,

            q => {
                tracing::debug!("could not serialize number {q} to enum Addressfamily");
                Self::Unspec
            }
        }
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Snapshot0Filestat {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Filestat {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Snapshot0Whence {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl wasmer::FromToNativeWasmType for Snapshot0Whence {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }

    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Cur,
            1 => Self::End,
            2 => Self::Set,

            q => {
                tracing::debug!("could not serialize number {q} to enum Snapshot0Whence");
                Self::Unknown
            }
        }
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Whence {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl wasmer::FromToNativeWasmType for Whence {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }

    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Set,
            1 => Self::Cur,
            2 => Self::End,

            q => {
                tracing::debug!("could not serialize number {q} to enum Whence");
                Self::Unknown
            }
        }
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Tty {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for OptionTag {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl wasmer::FromToNativeWasmType for OptionTag {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }

    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::None,
            1 => Self::Some,

            q => {
                tracing::debug!("could not serialize number {q} to enum OptionTag");
                Self::None
            }
        }
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for OptionPid {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for ProcessHandles {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for OptionFd {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for PrestatUDir {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for PrestatU {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for PipeHandles {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for StdioMode {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl wasmer::FromToNativeWasmType for StdioMode {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }

    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Piped,
            1 => Self::Inherit,
            2 => Self::Null,
            3 => Self::Log,

            q => {
                tracing::debug!("could not serialize number {q} to enum StdioMode");
                Self::Null
            }
        }
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for SockProto {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl wasmer::FromToNativeWasmType for SockProto {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }

    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Ip,
            1 => Self::Icmp,
            2 => Self::Igmp,
            3 => Self::ProtoThree,
            4 => Self::Ipip,
            5 => Self::ProtoFive,
            6 => Self::Tcp,
            7 => Self::ProtoSeven,
            8 => Self::Egp,
            9 => Self::ProtoNine,
            10 => Self::ProtoTen,
            11 => Self::ProtoEleven,
            12 => Self::Pup,
            13 => Self::ProtoThirteen,
            14 => Self::ProtoFourteen,
            15 => Self::ProtoFifteen,
            16 => Self::ProtoSixteen,
            17 => Self::Udp,
            18 => Self::ProtoEighteen,
            19 => Self::ProtoNineteen,
            20 => Self::ProtoTwenty,
            21 => Self::ProtoTwentyone,
            22 => Self::Idp,
            23 => Self::ProtoTwentythree,
            24 => Self::ProtoTwentyfour,
            25 => Self::ProtoTwentyfive,
            26 => Self::ProtoTwentysix,
            27 => Self::ProtoTwentyseven,
            28 => Self::ProtoTwentyeight,
            29 => Self::ProtoTp,
            30 => Self::ProtoThirty,
            31 => Self::ProtoThirtyone,
            32 => Self::ProtoThirtytwo,
            33 => Self::Dccp,
            34 => Self::ProtoThirtyfour,
            35 => Self::ProtoThirtyfive,
            36 => Self::ProtoThirtysix,
            37 => Self::ProtoThirtyseven,
            38 => Self::ProtoThirtyeight,
            39 => Self::ProtoThirtynine,
            40 => Self::ProtoFourty,
            41 => Self::Ipv6,
            42 => Self::ProtoFourtytwo,
            43 => Self::Routing,
            44 => Self::Fragment,
            45 => Self::ProtoFourtyfive,
            46 => Self::Rsvp,
            47 => Self::Gre,
            48 => Self::ProtoFourtyeight,
            49 => Self::ProtoFourtynine,
            50 => Self::Esp,
            51 => Self::Ah,
            52 => Self::ProtoFiftytwo,
            53 => Self::ProtoFiftythree,
            54 => Self::ProtoFiftyfour,
            55 => Self::ProtoFiftyfive,
            56 => Self::ProtoFiftysix,
            57 => Self::ProtoFiftyseven,
            58 => Self::Icmpv6,
            59 => Self::None,
            60 => Self::Dstopts,
            61 => Self::ProtoSixtyone,
            62 => Self::ProtoSixtytwo,
            63 => Self::ProtoSixtythree,
            64 => Self::ProtoSixtyfour,
            65 => Self::ProtoSixtyfive,
            66 => Self::ProtoSixtysix,
            67 => Self::ProtoSixtyseven,
            68 => Self::ProtoSixtyeight,
            69 => Self::ProtoSixtynine,
            70 => Self::ProtoSeventy,
            71 => Self::ProtoSeventyone,
            72 => Self::ProtoSeventytwo,
            73 => Self::ProtoSeventythree,
            74 => Self::ProtoSeventyfour,
            75 => Self::ProtoSeventyfive,
            76 => Self::ProtoSeventysix,
            77 => Self::ProtoSeventyseven,
            78 => Self::ProtoSeventyeight,
            79 => Self::ProtoSeventynine,
            80 => Self::ProtoEighty,
            81 => Self::ProtoEightyone,
            82 => Self::ProtoEightytwo,
            83 => Self::ProtoEightythree,
            84 => Self::ProtoEightyfour,
            85 => Self::ProtoEightyfive,
            86 => Self::ProtoEightysix,
            87 => Self::ProtoEightyseven,
            88 => Self::ProtoEightyeight,
            89 => Self::ProtoEightynine,
            90 => Self::ProtoNinety,
            91 => Self::ProtoNinetyone,
            92 => Self::Mtp,
            93 => Self::ProtoNinetythree,
            94 => Self::Beetph,
            95 => Self::ProtoNinetyfive,
            96 => Self::ProtoNinetysix,
            97 => Self::ProtoNineetyseven,
            98 => Self::Encap,
            99 => Self::ProtoNinetynine,
            100 => Self::ProtoOnehundred,
            101 => Self::ProtoOnehundredandone,
            102 => Self::ProtoOnehundredandtwo,
            103 => Self::Pim,
            104 => Self::ProtoOnehundredandfour,
            105 => Self::ProtoOnehundredandfive,
            106 => Self::ProtoOnehundredandsix,
            107 => Self::ProtoOnehundredandseven,
            108 => Self::Comp,
            109 => Self::ProtoOnehundredandnine,
            110 => Self::ProtoOnehundredandten,
            111 => Self::ProtoOnehundredandeleven,
            112 => Self::ProtoOnehundredandtwelve,
            113 => Self::ProtoOnehundredandthirteen,
            114 => Self::ProtoOnehundredandfourteen,
            115 => Self::ProtoOnehundredandfifteen,
            116 => Self::ProtoOnehundredandsixteen,
            117 => Self::ProtoOnehundredandseventeen,
            118 => Self::ProtoOnehundredandeighteen,
            119 => Self::ProtoOnehundredandnineteen,
            120 => Self::ProtoOnehundredandtwenty,
            121 => Self::ProtoOnehundredandtwentyone,
            122 => Self::ProtoOnehundredandtwentytwo,
            123 => Self::ProtoOnehundredandtwentythree,
            124 => Self::ProtoOnehundredandtwentyfour,
            125 => Self::ProtoOnehundredandtwentyfive,
            126 => Self::ProtoOnehundredandtwentysix,
            127 => Self::ProtoOnehundredandtwentyseven,
            128 => Self::ProtoOnehundredandtwentyeight,
            129 => Self::ProtoOnehundredandtwentynine,
            130 => Self::ProtoOnehundredandthirty,
            131 => Self::ProtoOnehundredandthirtyone,
            132 => Self::Sctp,
            133 => Self::ProtoOnehundredandthirtythree,
            134 => Self::ProtoOnehundredandthirtyfour,
            135 => Self::Mh,
            136 => Self::Udplite,
            137 => Self::Mpls,
            138 => Self::ProtoOnehundredandthirtyeight,
            139 => Self::ProtoOnehundredandthirtynine,
            140 => Self::ProtoOnehundredandfourty,
            141 => Self::ProtoOnehundredandfourtyone,
            142 => Self::ProtoOnehundredandfourtytwo,
            143 => Self::Ethernet,
            144 => Self::ProtoOnehundredandfourtyfour,
            145 => Self::ProtoOnehundredandfourtyfive,
            146 => Self::ProtoOnehundredandfourtysix,
            147 => Self::ProtoOnehundredandfourtyseven,
            148 => Self::ProtoOnehundredandfourtyeight,
            149 => Self::ProtoOnehundredandfourtynine,
            150 => Self::ProtoOnehundredandfifty,
            151 => Self::ProtoOnehundredandfiftyone,
            152 => Self::ProtoOnehundredandfiftytwo,
            153 => Self::ProtoOnehundredandfiftythree,
            154 => Self::ProtoOnehundredandfiftyfour,
            155 => Self::ProtoOnehundredandfiftyfive,
            156 => Self::ProtoOnehundredandfiftysix,
            157 => Self::ProtoOnehundredandfiftyseven,
            158 => Self::ProtoOnehundredandfiftyeight,
            159 => Self::ProtoOnehundredandfiftynine,
            160 => Self::ProtoOnehundredandsixty,
            161 => Self::ProtoOnehundredandsixtyone,
            162 => Self::ProtoOnehundredandsixtytwo,
            163 => Self::ProtoOnehundredandsixtythree,
            164 => Self::ProtoOnehundredandsixtyfour,
            165 => Self::ProtoOnehundredandsixtyfive,
            166 => Self::ProtoOnehundredandsixtysix,
            167 => Self::ProtoOnehundredandsixtyseven,
            168 => Self::ProtoOnehundredandsixtyeight,
            169 => Self::ProtoOnehundredandsixtynine,
            170 => Self::ProtoOnehundredandseventy,
            171 => Self::ProtoOnehundredandseventyone,
            172 => Self::ProtoOnehundredandseventytwo,
            173 => Self::ProtoOnehundredandseventythree,
            174 => Self::ProtoOnehundredandseventyfour,
            175 => Self::ProtoOnehundredandseventyfive,
            176 => Self::ProtoOnehundredandseventysix,
            177 => Self::ProtoOnehundredandseventyseven,
            178 => Self::ProtoOnehundredandseventyeight,
            179 => Self::ProtoOnehundredandseventynine,
            180 => Self::ProtoOnehundredandeighty,
            181 => Self::ProtoOnehundredandeightyone,
            182 => Self::ProtoOnehundredandeightytwo,
            183 => Self::ProtoOnehundredandeightythree,
            184 => Self::ProtoOnehundredandeightyfour,
            185 => Self::ProtoOnehundredandeightyfive,
            186 => Self::ProtoOnehundredandeightysix,
            187 => Self::ProtoOnehundredandeightyseven,
            188 => Self::ProtoOnehundredandeightyeight,
            189 => Self::ProtoOnehundredandeightynine,
            190 => Self::ProtoOnehundredandninety,
            191 => Self::ProtoOnehundredandninetyone,
            192 => Self::ProtoOnehundredandninetytwo,
            193 => Self::ProtoOnehundredandninetythree,
            194 => Self::ProtoOnehundredandninetyfour,
            195 => Self::ProtoOnehundredandninetyfive,
            196 => Self::ProtoOnehundredandninetysix,
            197 => Self::ProtoOnehundredandninetyseven,
            198 => Self::ProtoOnehundredandninetyeight,
            199 => Self::ProtoOnehundredandninetynine,
            200 => Self::ProtoTwohundred,
            201 => Self::ProtoTwohundredandone,
            202 => Self::ProtoTwohundredandtwo,
            203 => Self::ProtoTwohundredandthree,
            204 => Self::ProtoTwohundredandfour,
            205 => Self::ProtoTwohundredandfive,
            206 => Self::ProtoTwohundredandsix,
            207 => Self::ProtoTwohundredandseven,
            208 => Self::ProtoTwohundredandeight,
            209 => Self::ProtoTwohundredandnine,
            210 => Self::ProtoTwohundredandten,
            211 => Self::ProtoTwohundredandeleven,
            212 => Self::ProtoTwohundredandtwelve,
            213 => Self::ProtoTwohundredandthirteen,
            214 => Self::ProtoTwohundredandfourteen,
            215 => Self::ProtoTwohundredandfifteen,
            216 => Self::ProtoTwohundredandsixteen,
            217 => Self::ProtoTwohundredandseventeen,
            218 => Self::ProtoTwohundredandeighteen,
            219 => Self::ProtoTwohundredandnineteen,
            220 => Self::ProtoTwohundredandtwenty,
            221 => Self::ProtoTwohundredandtwentyone,
            222 => Self::ProtoTwohundredandtwentytwo,
            223 => Self::ProtoTwohundredandtwentythree,
            224 => Self::ProtoTwohundredandtwentyfour,
            225 => Self::ProtoTwohundredandtwentyfive,
            226 => Self::ProtoTwohundredandtwentysix,
            227 => Self::ProtoTwohundredandtwentyseven,
            228 => Self::ProtoTwohundredandtwentyeight,
            229 => Self::ProtoTwohundredandtwentynine,
            230 => Self::ProtoTwohundredandthirty,
            231 => Self::ProtoTwohundredandthirtyone,
            232 => Self::ProtoTwohundredandthirtytwo,
            233 => Self::ProtoTwohundredandthirtythree,
            234 => Self::ProtoTwohundredandthirtyfour,
            235 => Self::ProtoTwohundredandthirtyfive,
            236 => Self::ProtoTwohundredandthirtysix,
            237 => Self::ProtoTwohundredandthirtyseven,
            238 => Self::ProtoTwohundredandthirtyeight,
            239 => Self::ProtoTwohundredandthirtynine,
            240 => Self::ProtoTwohundredandfourty,
            241 => Self::ProtoTwohundredandfourtyone,
            242 => Self::ProtoTwohundredandfourtytwo,
            243 => Self::ProtoTwohundredandfourtythree,
            244 => Self::ProtoTwohundredandfourtyfour,
            245 => Self::ProtoTwohundredandfourtyfive,
            246 => Self::ProtoTwohundredandfourtysix,
            247 => Self::ProtoTwohundredandfourtyseven,
            248 => Self::ProtoTwohundredandfourtyeight,
            249 => Self::ProtoTwohundredandfourtynine,
            250 => Self::ProtoTwohundredandfifty,
            251 => Self::ProtoTwohundredandfiftyone,
            252 => Self::ProtoTwohundredandfiftytwo,
            253 => Self::ProtoTwohundredandfiftythree,
            254 => Self::ProtoTwohundredandfiftyfour,
            255 => Self::ProtoRaw,
            256 => Self::ProtoTwohundredandfiftysix,
            257 => Self::ProtoTwohundredandfiftyseven,
            258 => Self::ProtoTwohundredandfiftyeight,
            259 => Self::ProtoTwohundredandfiftynine,
            260 => Self::ProtoTwohundredandsixty,
            261 => Self::ProtoTwohundredandsixtyone,
            262 => Self::Mptcp,
            263 => Self::Max,

            q => {
                tracing::debug!("could not serialize number {q} to enum SockProto");
                Self::None
            }
        }
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Bool {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl wasmer::FromToNativeWasmType for Bool {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }

    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::False,
            1 => Self::True,

            q => {
                tracing::debug!("could not serialize number {q} to enum Bool");
                Self::False
            }
        }
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for OptionTimestamp {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Signal {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl wasmer::FromToNativeWasmType for Signal {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }

    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Signone,
            1 => Self::Sighup,
            2 => Self::Sigint,
            3 => Self::Sigquit,
            4 => Self::Sigill,
            5 => Self::Sigtrap,
            6 => Self::Sigabrt,
            7 => Self::Sigbus,
            8 => Self::Sigfpe,
            9 => Self::Sigkill,
            10 => Self::Sigusr1,
            11 => Self::Sigsegv,
            12 => Self::Sigusr2,
            13 => Self::Sigpipe,
            14 => Self::Sigalrm,
            15 => Self::Sigterm,
            16 => Self::Sigstkflt,
            17 => Self::Sigchld,
            18 => Self::Sigcont,
            19 => Self::Sigstop,
            20 => Self::Sigtstp,
            21 => Self::Sigttin,
            22 => Self::Sigttou,
            23 => Self::Sigurg,
            24 => Self::Sigxcpu,
            25 => Self::Sigxfsz,
            26 => Self::Sigvtalrm,
            27 => Self::Sigprof,
            28 => Self::Sigwinch,
            29 => Self::Sigpoll,
            30 => Self::Sigpwr,
            31 => Self::Sigsys,

            _ => Self::Signone,
        }
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for AddrUnspec {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for AddrUnspecPort {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for CidrUnspec {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for HttpHandles {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for HttpStatus {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Timeout {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl wasmer::FromToNativeWasmType for Timeout {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }

    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Read,
            1 => Self::Write,
            2 => Self::Connect,
            3 => Self::Accept,

            q => {
                tracing::debug!("could not serialize number {q} to enum Timeout");
                Self::Unknown
            }
        }
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for JoinFlags {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for JoinStatusType {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl wasmer::FromToNativeWasmType for JoinStatusType {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }

    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Nothing,
            1 => Self::ExitNormal,
            2 => Self::ExitSignal,
            3 => Self::Stopped,

            q => {
                tracing::debug!("could not serialize number {q} to enum JoinStatusType");
                Self::Nothing
            }
        }
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for ErrnoSignal {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}
