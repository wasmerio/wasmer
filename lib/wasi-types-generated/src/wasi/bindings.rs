#[allow(clippy::all)]
pub mod wasi {
  #[allow(unused_imports)]
  use wit_bindgen_wasmer::{anyhow, wasmer};
  /// Non-negative file size or length of a region within a file.
  pub type Filesize = u64;
  /// Timestamp in nanoseconds.
  pub type Timestamp = u64;
  /// A file descriptor handle.
  pub type Fd = u32;
  /// A reference to the offset of a directory entry.
  pub type Dircookie = u64;
  /// The type for the `dirent::d-namlen` field of `dirent` struct.
  pub type Dirnamlen = u32;
  /// File serial number that is unique within its file system.
  pub type Inode = u64;
  /// Identifiers for clocks, snapshot0 version.
  #[repr(u8)]
  #[derive(Clone, Copy, PartialEq, Eq)]
  pub enum Snapshot0Clockid {
    /// The clock measuring real time. Time value zero corresponds with
    /// 1970-01-01T00:00:00Z.
    Realtime,
    /// The store-wide monotonic clock, which is defined as a clock measuring
    /// real time, whose value cannot be adjusted and which cannot have negative
    /// clock jumps. The epoch of this clock is undefined. The absolute time
    /// value of this clock therefore has no meaning.
    Monotonic,
    /// The CPU-time clock associated with the current process.
    ProcessCputimeId,
    /// The CPU-time clock associated with the current thread.
    ThreadCputimeId,
  }
  impl core::fmt::Debug for Snapshot0Clockid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        Snapshot0Clockid::Realtime => {
          f.debug_tuple("Snapshot0Clockid::Realtime").finish()
        }
        Snapshot0Clockid::Monotonic => {
          f.debug_tuple("Snapshot0Clockid::Monotonic").finish()
        }
        Snapshot0Clockid::ProcessCputimeId => {
          f.debug_tuple("Snapshot0Clockid::ProcessCputimeId").finish()
        }
        Snapshot0Clockid::ThreadCputimeId => {
          f.debug_tuple("Snapshot0Clockid::ThreadCputimeId").finish()
        }
      }
    }
  }
  /// Identifiers for clocks.
  #[repr(u8)]
  #[derive(Clone, Copy, PartialEq, Eq)]
  pub enum Clockid {
    /// The clock measuring real time. Time value zero corresponds with
    /// 1970-01-01T00:00:00Z.
    Realtime,
    /// The store-wide monotonic clock, which is defined as a clock measuring
    /// real time, whose value cannot be adjusted and which cannot have negative
    /// clock jumps. The epoch of this clock is undefined. The absolute time
    /// value of this clock therefore has no meaning.
    Monotonic,
  }
  impl core::fmt::Debug for Clockid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        Clockid::Realtime => {
          f.debug_tuple("Clockid::Realtime").finish()
        }
        Clockid::Monotonic => {
          f.debug_tuple("Clockid::Monotonic").finish()
        }
      }
    }
  }
  /// Error codes returned by functions.
  /// Not all of these error codes are returned by the functions provided by this
  /// API; some are used in higher-level library layers, and others are provided
  /// merely for alignment with POSIX.
  #[repr(u8)]
  #[derive(Clone, Copy, PartialEq, Eq)]
  pub enum Errno {
    /// No error occurred. System call completed successfully.
    Success,
    /// Argument list too long.
    Toobig,
    /// Permission denied.
    Access,
    /// Address in use.
    Addrinuse,
    /// Address not available.
    Addrnotavail,
    /// Address family not supported.
    Afnosupport,
    /// Resource unavailable, or operation would block.
    Again,
    /// Connection already in progress.
    Already,
    /// Bad file descriptor.
    Badf,
    /// Bad message.
    Badmsg,
    /// Device or resource busy.
    Busy,
    /// Operation canceled.
    Canceled,
    /// No child processes.
    Child,
    /// Connection aborted.
    Connaborted,
    /// Connection refused.
    Connrefused,
    /// Connection reset.
    Connreset,
    /// Resource deadlock would occur.
    Deadlk,
    /// Destination address required.
    Destaddrreq,
    /// Mathematics argument out of domain of function.
    Dom,
    /// Reserved.
    Dquot,
    /// File exists.
    Exist,
    /// Bad address.
    Fault,
    /// File too large.
    Fbig,
    /// Host is unreachable.
    Hostunreach,
    /// Identifier removed.
    Idrm,
    /// Illegal byte sequence.
    Ilseq,
    /// Operation in progress.
    Inprogress,
    /// Interrupted function.
    Intr,
    /// Invalid argument.
    Inval,
    /// I/O error.
    Io,
    /// Socket is connected.
    Isconn,
    /// Is a directory.
    Isdir,
    /// Too many levels of symbolic links.
    Loop,
    /// File descriptor value too large.
    Mfile,
    /// Too many links.
    Mlink,
    /// Message too large.
    Msgsize,
    /// Reserved.
    Multihop,
    /// Filename too long.
    Nametoolong,
    /// Network is down.
    Netdown,
    /// Connection aborted by network.
    Netreset,
    /// Network unreachable.
    Netunreach,
    /// Too many files open in system.
    Nfile,
    /// No buffer space available.
    Nobufs,
    /// No such device.
    Nodev,
    /// No such file or directory.
    Noent,
    /// Executable file format error.
    Noexec,
    /// No locks available.
    Nolck,
    /// Reserved.
    Nolink,
    /// Not enough space.
    Nomem,
    /// No message of the desired type.
    Nomsg,
    /// Protocol not available.
    Noprotoopt,
    /// No space left on device.
    Nospc,
    /// Function not supported.
    Nosys,
    /// The socket is not connected.
    Notconn,
    /// Not a directory or a symbolic link to a directory.
    Notdir,
    /// Directory not empty.
    Notempty,
    /// State not recoverable.
    Notrecoverable,
    /// Not a socket.
    Notsock,
    /// Not supported, or operation not supported on socket.
    Notsup,
    /// Inappropriate I/O control operation.
    Notty,
    /// No such device or address.
    Nxio,
    /// Value too large to be stored in data type.
    Overflow,
    /// Previous owner died.
    Ownerdead,
    /// Operation not permitted.
    Perm,
    /// Broken pipe.
    Pipe,
    /// Protocol error.
    Proto,
    /// Protocol not supported.
    Protonosupport,
    /// Protocol wrong type for socket.
    Prototype,
    /// Result too large.
    Range,
    /// Read-only file system.
    Rofs,
    /// Invalid seek.
    Spipe,
    /// No such process.
    Srch,
    /// Reserved.
    Stale,
    /// Connection timed out.
    Timedout,
    /// Text file busy.
    Txtbsy,
    /// Cross-device link.
    Xdev,
    /// Extension: Capabilities insufficient.
    Notcapable,
  }
  impl Errno{
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
      }
    }
  }
  impl core::fmt::Debug for Errno{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("Errno")
      .field("code", &(*self as i32))
      .field("name", &self.name())
      .field("message", &self.message())
      .finish()
    }
  }
  impl core::fmt::Display for Errno{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      write!(f, "{} (error {})", self.name(), *self as i32)}
  }
  
  impl std::error::Error for Errno{}
  #[repr(u8)]
  #[derive(Clone, Copy, PartialEq, Eq)]
  pub enum BusErrno {
    /// No error occurred. Call completed successfully.
    Success,
    /// Failed during serialization
    Ser,
    /// Failed during deserialization
    Des,
    /// Invalid WAPM process
    Wapm,
    /// Failed to fetch the WAPM process
    Fetch,
    /// Failed to compile the WAPM process
    Compile,
    /// Invalid ABI
    Abi,
    /// Call was aborted
    Aborted,
    /// Bad handle
    Badhandle,
    /// Invalid topic
    Topic,
    /// Invalid callback
    Badcb,
    /// Call is unsupported
    Unsupported,
    /// Bad request
    Badrequest,
    /// Access denied
    Denied,
    /// Internal error has occured
    Internal,
    /// Memory allocation failed
    Alloc,
    /// Invocation has failed
    Invoke,
    /// Already consumed
    Consumed,
    /// Memory access violation
    Memviolation,
    /// Some other unhandled error. If you see this, it's probably a bug.
    Unknown,
  }
  impl BusErrno{
    pub fn name(&self) -> &'static str {
      match self {
        BusErrno::Success => "success",
        BusErrno::Ser => "ser",
        BusErrno::Des => "des",
        BusErrno::Wapm => "wapm",
        BusErrno::Fetch => "fetch",
        BusErrno::Compile => "compile",
        BusErrno::Abi => "abi",
        BusErrno::Aborted => "aborted",
        BusErrno::Badhandle => "badhandle",
        BusErrno::Topic => "topic",
        BusErrno::Badcb => "badcb",
        BusErrno::Unsupported => "unsupported",
        BusErrno::Badrequest => "badrequest",
        BusErrno::Denied => "denied",
        BusErrno::Internal => "internal",
        BusErrno::Alloc => "alloc",
        BusErrno::Invoke => "invoke",
        BusErrno::Consumed => "consumed",
        BusErrno::Memviolation => "memviolation",
        BusErrno::Unknown => "unknown",
      }
    }
    pub fn message(&self) -> &'static str {
      match self {
        BusErrno::Success => "No error occurred. Call completed successfully.",
        BusErrno::Ser => "Failed during serialization",
        BusErrno::Des => "Failed during deserialization",
        BusErrno::Wapm => "Invalid WAPM process",
        BusErrno::Fetch => "Failed to fetch the WAPM process",
        BusErrno::Compile => "Failed to compile the WAPM process",
        BusErrno::Abi => "Invalid ABI",
        BusErrno::Aborted => "Call was aborted",
        BusErrno::Badhandle => "Bad handle",
        BusErrno::Topic => "Invalid topic",
        BusErrno::Badcb => "Invalid callback",
        BusErrno::Unsupported => "Call is unsupported",
        BusErrno::Badrequest => "Bad request",
        BusErrno::Denied => "Access denied",
        BusErrno::Internal => "Internal error has occured",
        BusErrno::Alloc => "Memory allocation failed",
        BusErrno::Invoke => "Invocation has failed",
        BusErrno::Consumed => "Already consumed",
        BusErrno::Memviolation => "Memory access violation",
        BusErrno::Unknown => "Some other unhandled error. If you see this, it's probably a bug.",
      }
    }
  }
  impl core::fmt::Debug for BusErrno{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("BusErrno")
      .field("code", &(*self as i32))
      .field("name", &self.name())
      .field("message", &self.message())
      .finish()
    }
  }
  impl core::fmt::Display for BusErrno{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      write!(f, "{} (error {})", self.name(), *self as i32)}
  }
  
  impl std::error::Error for BusErrno{}
  wit_bindgen_wasmer::bitflags::bitflags! {
    /// File descriptor rights, determining which actions may be performed.
    pub struct Rights: u64 {/// The right to invoke `fd_datasync`.
      /// 
      /// If `rights::path_open` is set, includes the right to invoke
      /// `path_open` with `fdflags::dsync`.
      const FD_DATASYNC = 1 << 0;
      /// The right to invoke `fd_read` and `sock_recv`.
      /// 
      /// If `rights::fd_seek` is set, includes the right to invoke `fd_pread`.
      const FD_READ = 1 << 1;
      /// The right to invoke `fd_seek`. This flag implies `rights::fd_tell`.
      const FD_SEEK = 1 << 2;
      /// The right to invoke `fd_fdstat_set_flags`.
      const FD_FDSTAT_SET_FLAGS = 1 << 3;
      /// The right to invoke `fd_sync`.
      /// 
      /// If `rights::path_open` is set, includes the right to invoke
      /// `path_open` with `fdflags::rsync` and `fdflags::dsync`.
      const FD_SYNC = 1 << 4;
      /// The right to invoke `fd_seek` in such a way that the file offset
      /// remains unaltered (i.e., `whence::cur` with offset zero), or to
      /// invoke `fd_tell`.
      const FD_TELL = 1 << 5;
      /// The right to invoke `fd_write` and `sock_send`.
      /// If `rights::fd_seek` is set, includes the right to invoke `fd_pwrite`.
      const FD_WRITE = 1 << 6;
      /// The right to invoke `fd_advise`.
      const FD_ADVISE = 1 << 7;
      /// The right to invoke `fd_allocate`.
      const FD_ALLOCATE = 1 << 8;
      /// The right to invoke `path_create_directory`.
      const PATH_CREATE_DIRECTORY = 1 << 9;
      /// If `rights::path_open` is set, the right to invoke `path_open` with `oflags::creat`.
      const PATH_CREATE_FILE = 1 << 10;
      /// The right to invoke `path_link` with the file descriptor as the
      /// source directory.
      const PATH_LINK_SOURCE = 1 << 11;
      /// The right to invoke `path_link` with the file descriptor as the
      /// target directory.
      const PATH_LINK_TARGET = 1 << 12;
      /// The right to invoke `path_open`.
      const PATH_OPEN = 1 << 13;
      /// The right to invoke `fd_readdir`.
      const FD_READDIR = 1 << 14;
      /// The right to invoke `path_readlink`.
      const PATH_READLINK = 1 << 15;
      /// The right to invoke `path_rename` with the file descriptor as the source directory.
      const PATH_RENAME_SOURCE = 1 << 16;
      /// The right to invoke `path_rename` with the file descriptor as the target directory.
      const PATH_RENAME_TARGET = 1 << 17;
      /// The right to invoke `path_filestat_get`.
      const PATH_FILESTAT_GET = 1 << 18;
      /// The right to change a file's size (there is no `path_filestat_set_size`).
      /// If `rights::path_open` is set, includes the right to invoke `path_open` with `oflags::trunc`.
      const PATH_FILESTAT_SET_SIZE = 1 << 19;
      /// The right to invoke `path_filestat_set_times`.
      const PATH_FILESTAT_SET_TIMES = 1 << 20;
      /// The right to invoke `fd_filestat_get`.
      const FD_FILESTAT_GET = 1 << 21;
      /// The right to invoke `fd_filestat_set_size`.
      const FD_FILESTAT_SET_SIZE = 1 << 22;
      /// The right to invoke `fd_filestat_set_times`.
      const FD_FILESTAT_SET_TIMES = 1 << 23;
      /// The right to invoke `path_symlink`.
      const PATH_SYMLINK = 1 << 24;
      /// The right to invoke `path_remove_directory`.
      const PATH_REMOVE_DIRECTORY = 1 << 25;
      /// The right to invoke `path_unlink_file`.
      const PATH_UNLINK_FILE = 1 << 26;
      /// If `rights::fd_read` is set, includes the right to invoke `poll_oneoff` to subscribe to `eventtype::fd_read`.
      /// If `rights::fd_write` is set, includes the right to invoke `poll_oneoff` to subscribe to `eventtype::fd_write`.
      const POLL_FD_READWRITE = 1 << 27;
      /// The right to invoke `sock_shutdown`.
      const SOCK_SHUTDOWN = 1 << 28;
      /// TODO: Found in wasmer-wasi-types rust project, but not in wasi-snapshot0
      const SOCK_ACCEPT = 1 << 29;
      /// TODO: Found in wasmer-wasi-types rust project, but not in wasi-snapshot0
      const SOCK_CONNECT = 1 << 30;
      /// TODO: Found in wasmer-wasi-types rust project, but not in wasi-snapshot0
      const SOCK_LISTEN = 1 << 31;
      /// TODO: Found in wasmer-wasi-types rust project, but not in wasi-snapshot0
      const SOCK_BIND = 1 << 32;
      /// TODO: Found in wasmer-wasi-types rust project, but not in wasi-snapshot0
      const SOCK_RECV = 1 << 33;
      /// TODO: Found in wasmer-wasi-types rust project, but not in wasi-snapshot0
      const SOCK_SEND = 1 << 34;
      /// TODO: Found in wasmer-wasi-types rust project, but not in wasi-snapshot0
      const SOCK_ADDR_LOCAL = 1 << 35;
      /// TODO: Found in wasmer-wasi-types rust project, but not in wasi-snapshot0
      const SOCK_ADDR_REMOTE = 1 << 36;
      /// TODO: Found in wasmer-wasi-types rust project, but not in wasi-snapshot0
      const SOCK_RECV_FROM = 1 << 37;
      /// TODO: Found in wasmer-wasi-types rust project, but not in wasi-snapshot0
      const SOCK_SEND_TO = 1 << 38;
    }
  }
  
  impl core::fmt::Display for Rights{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.write_str("Rights(")?;
      core::fmt::Debug::fmt(self, f)?;
      f.write_str(" (0x")?;
      core::fmt::LowerHex::fmt(&self.bits, f)?;
      f.write_str("))")?;
      Ok(())}
  }
  
  /// The type of a file descriptor or file.
  #[repr(u8)]
  #[derive(Clone, Copy, PartialEq, Eq)]
  pub enum Filetype {
    /// The type of the file descriptor or file is unknown or is different from any of the other types specified.
    Unknown,
    /// The file descriptor or file refers to a block device inode.
    BlockDevice,
    /// The file descriptor or file refers to a character device inode.
    CharacterDevice,
    /// The file descriptor or file refers to a directory inode.
    Directory,
    /// The file descriptor or file refers to a regular file inode.
    RegularFile,
    /// The file descriptor or file refers to a datagram socket.
    SocketDgram,
    /// The file descriptor or file refers to a byte-stream socket.
    SocketStream,
    /// The file refers to a symbolic link inode.
    SymbolicLink,
    /// The file descriptor or file refers to a FIFO.
    Fifo,
  }
  impl core::fmt::Debug for Filetype {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        Filetype::Unknown => {
          f.debug_tuple("Filetype::Unknown").finish()
        }
        Filetype::BlockDevice => {
          f.debug_tuple("Filetype::BlockDevice").finish()
        }
        Filetype::CharacterDevice => {
          f.debug_tuple("Filetype::CharacterDevice").finish()
        }
        Filetype::Directory => {
          f.debug_tuple("Filetype::Directory").finish()
        }
        Filetype::RegularFile => {
          f.debug_tuple("Filetype::RegularFile").finish()
        }
        Filetype::SocketDgram => {
          f.debug_tuple("Filetype::SocketDgram").finish()
        }
        Filetype::SocketStream => {
          f.debug_tuple("Filetype::SocketStream").finish()
        }
        Filetype::SymbolicLink => {
          f.debug_tuple("Filetype::SymbolicLink").finish()
        }
        Filetype::Fifo => {
          f.debug_tuple("Filetype::Fifo").finish()
        }
      }
    }
  }
  /// A directory entry.
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct Dirent {
    /// The offset of the next directory entry stored in this directory.
    pub d_next: Dircookie,
    /// The serial number of the file referred to by this directory entry.
    pub d_ino: Inode,
    /// The type of the file referred to by this directory entry.
    pub d_type: Filetype,
    /// The length of the name of the directory entry.
    pub d_namlen: Dirnamlen,
  }
  impl core::fmt::Debug for Dirent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("Dirent").field("d-next", &self.d_next).field("d-ino", &self.d_ino).field("d-type", &self.d_type).field("d-namlen", &self.d_namlen).finish()}
  }
  /// File or memory access pattern advisory information.
  #[repr(u8)]
  #[derive(Clone, Copy, PartialEq, Eq)]
  pub enum Advice {
    /// The application has no advice to give on its behavior with respect to the specified data.
    Normal,
    /// The application expects to access the specified data sequentially from lower offsets to higher offsets.
    Sequential,
    /// The application expects to access the specified data in a random order.
    Random,
    /// The application expects to access the specified data in the near future.
    Willneed,
    /// The application expects that it will not access the specified data in the near future.
    Dontneed,
    /// The application expects to access the specified data once and then not reuse it thereafter.
    Noreuse,
  }
  impl core::fmt::Debug for Advice {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        Advice::Normal => {
          f.debug_tuple("Advice::Normal").finish()
        }
        Advice::Sequential => {
          f.debug_tuple("Advice::Sequential").finish()
        }
        Advice::Random => {
          f.debug_tuple("Advice::Random").finish()
        }
        Advice::Willneed => {
          f.debug_tuple("Advice::Willneed").finish()
        }
        Advice::Dontneed => {
          f.debug_tuple("Advice::Dontneed").finish()
        }
        Advice::Noreuse => {
          f.debug_tuple("Advice::Noreuse").finish()
        }
      }
    }
  }
  wit_bindgen_wasmer::bitflags::bitflags! {
    /// File descriptor flags.
    pub struct Fdflags: u8 {/// Append mode: Data written to the file is always appended to the file's end.
      const APPEND = 1 << 0;
      /// Write according to synchronized I/O data integrity completion. Only the data stored in the file is synchronized.
      const DSYNC = 1 << 1;
      /// Non-blocking mode.
      const NONBLOCK = 1 << 2;
      /// Synchronized read I/O operations.
      const RSYNC = 1 << 3;
      /// Write according to synchronized I/O file integrity completion. In
      /// addition to synchronizing the data stored in the file, the implementation
      /// may also synchronously update the file's metadata.
      const SYNC = 1 << 4;
    }
  }
  
  impl core::fmt::Display for Fdflags{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.write_str("Fdflags(")?;
      core::fmt::Debug::fmt(self, f)?;
      f.write_str(" (0x")?;
      core::fmt::LowerHex::fmt(&self.bits, f)?;
      f.write_str("))")?;
      Ok(())}
  }
  
  /// File descriptor attributes.
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct Fdstat {
    /// File type.
    pub fs_filetype: Filetype,
    /// File descriptor flags.
    pub fs_flags: Fdflags,
    /// Rights that apply to this file descriptor.
    pub fs_rights_base: Rights,
    /// Maximum set of rights that may be installed on new file descriptors that
    /// are created through this file descriptor, e.g., through `path_open`.
    pub fs_rights_inheriting: Rights,
  }
  impl core::fmt::Debug for Fdstat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("Fdstat").field("fs-filetype", &self.fs_filetype).field("fs-flags", &self.fs_flags).field("fs-rights-base", &self.fs_rights_base).field("fs-rights-inheriting", &self.fs_rights_inheriting).finish()}
  }
  wit_bindgen_wasmer::bitflags::bitflags! {
    /// Which file time attributes to adjust.
    pub struct Fstflags: u8 {/// Adjust the last data access timestamp to the value stored in `filestat::atim`.
      const ATIM = 1 << 0;
      /// Adjust the last data access timestamp to the time of clock `clockid::realtime`.
      const ATIM_NOW = 1 << 1;
      /// Adjust the last data modification timestamp to the value stored in `filestat::mtim`.
      const MTIM = 1 << 2;
      /// Adjust the last data modification timestamp to the time of clock `clockid::realtime`.
      const MTIM_NOW = 1 << 3;
    }
  }
  
  impl core::fmt::Display for Fstflags{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.write_str("Fstflags(")?;
      core::fmt::Debug::fmt(self, f)?;
      f.write_str(" (0x")?;
      core::fmt::LowerHex::fmt(&self.bits, f)?;
      f.write_str("))")?;
      Ok(())}
  }
  
  wit_bindgen_wasmer::bitflags::bitflags! {
    /// Flags determining the method of how paths are resolved.
    pub struct Lookup: u8 {/// As long as the resolved path corresponds to a symbolic link, it is expanded.
      const SYMLINK_FOLLOW = 1 << 0;
    }
  }
  
  impl core::fmt::Display for Lookup{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.write_str("Lookup(")?;
      core::fmt::Debug::fmt(self, f)?;
      f.write_str(" (0x")?;
      core::fmt::LowerHex::fmt(&self.bits, f)?;
      f.write_str("))")?;
      Ok(())}
  }
  
  wit_bindgen_wasmer::bitflags::bitflags! {
    /// Open flags used by `path_open`.
    pub struct Oflags: u8 {/// Create file if it does not exist.
      const CREATE = 1 << 0;
      /// Fail if not a directory.
      const DIRECTORY = 1 << 1;
      /// Fail if file already exists.
      const EXCL = 1 << 2;
      /// Truncate file to size 0.
      const TRUNC = 1 << 3;
    }
  }
  
  impl core::fmt::Display for Oflags{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.write_str("Oflags(")?;
      core::fmt::Debug::fmt(self, f)?;
      f.write_str(" (0x")?;
      core::fmt::LowerHex::fmt(&self.bits, f)?;
      f.write_str("))")?;
      Ok(())}
  }
  
  /// User-provided value that may be attached to objects that is retained when
  /// extracted from the implementation.
  pub type Userdata = u64;
  /// Type of a subscription to an event or its occurrence.
  #[repr(u8)]
  #[derive(Clone, Copy, PartialEq, Eq)]
  pub enum Eventtype {
    /// The time value of clock `subscription_clock::id` has
    /// reached timestamp `subscription_clock::timeout`.
    Clock,
    /// File descriptor `subscription_fd_readwrite::fd` has data
    /// available for reading. This event always triggers for regular files.
    FdRead,
    /// File descriptor `subscription_fd_readwrite::fd` has capacity
    /// available for writing. This event always triggers for regular files.
    FdWrite,
  }
  impl core::fmt::Debug for Eventtype {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        Eventtype::Clock => {
          f.debug_tuple("Eventtype::Clock").finish()
        }
        Eventtype::FdRead => {
          f.debug_tuple("Eventtype::FdRead").finish()
        }
        Eventtype::FdWrite => {
          f.debug_tuple("Eventtype::FdWrite").finish()
        }
      }
    }
  }
  wit_bindgen_wasmer::bitflags::bitflags! {
    /// Flags determining how to interpret the timestamp provided in
    /// `subscription-clock::timeout`.
    pub struct Subclockflags: u8 {/// If set, treat the timestamp provided in
      /// `subscription-clock::timeout` as an absolute timestamp of clock
      /// `subscription-clock::id`. If clear, treat the timestamp
      /// provided in `subscription-clock::timeout` relative to the
      /// current time value of clock `subscription-clock::id`.
      const SUBSCRIPTION_CLOCK_ABSTIME = 1 << 0;
    }
  }
  
  impl core::fmt::Display for Subclockflags{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.write_str("Subclockflags(")?;
      core::fmt::Debug::fmt(self, f)?;
      f.write_str(" (0x")?;
      core::fmt::LowerHex::fmt(&self.bits, f)?;
      f.write_str("))")?;
      Ok(())}
  }
  
  /// The contents of a `subscription` when type is `eventtype::clock`.
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct Snapshot0SubscriptionClock {
    /// The user-defined unique identifier of the clock.
    pub identifier: Userdata,
    /// The clock against which to compare the timestamp.
    pub id: Snapshot0Clockid,
    /// The absolute or relative timestamp.
    pub timeout: Timestamp,
    /// The amount of time that the implementation may wait additionally
    /// to coalesce with other events.
    pub precision: Timestamp,
    /// Flags specifying whether the timeout is absolute or relative
    pub flags: Subclockflags,
  }
  impl core::fmt::Debug for Snapshot0SubscriptionClock {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("Snapshot0SubscriptionClock").field("identifier", &self.identifier).field("id", &self.id).field("timeout", &self.timeout).field("precision", &self.precision).field("flags", &self.flags).finish()}
  }
  /// The contents of a `subscription` when type is `eventtype::clock`.
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct SubscriptionClock {
    /// The clock against which to compare the timestamp.
    pub clock_id: Clockid,
    /// The absolute or relative timestamp.
    pub timeout: Timestamp,
    /// The amount of time that the implementation may wait additionally
    /// to coalesce with other events.
    pub precision: Timestamp,
    /// Flags specifying whether the timeout is absolute or relative
    pub flags: Subclockflags,
  }
  impl core::fmt::Debug for SubscriptionClock {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("SubscriptionClock").field("clock-id", &self.clock_id).field("timeout", &self.timeout).field("precision", &self.precision).field("flags", &self.flags).finish()}
  }
  /// Identifiers for preopened capabilities.
  #[repr(u8)]
  #[derive(Clone, Copy, PartialEq, Eq)]
  pub enum Preopentype {
    /// A pre-opened directory.
    Dir,
  }
  impl core::fmt::Debug for Preopentype {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        Preopentype::Dir => {
          f.debug_tuple("Preopentype::Dir").finish()
        }
      }
    }
  }
  wit_bindgen_wasmer::bitflags::bitflags! {
    /// The state of the file descriptor subscribed to with
    /// `eventtype::fd_read` or `eventtype::fd_write`.
    pub struct Eventrwflags: u8 {/// The peer of this socket has closed or disconnected.
      const FD_READWRITE_HANGUP = 1 << 0;
    }
  }
  
  impl core::fmt::Display for Eventrwflags{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.write_str("Eventrwflags(")?;
      core::fmt::Debug::fmt(self, f)?;
      f.write_str(" (0x")?;
      core::fmt::LowerHex::fmt(&self.bits, f)?;
      f.write_str("))")?;
      Ok(())}
  }
  
  /// The contents of an `event` for the `eventtype::fd_read` and
  /// `eventtype::fd_write` variants
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct EventFdReadwrite {
    /// The number of bytes available for reading or writing.
    pub nbytes: Filesize,
    /// The state of the file descriptor.
    pub flags: Eventrwflags,
  }
  impl core::fmt::Debug for EventFdReadwrite {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("EventFdReadwrite").field("nbytes", &self.nbytes).field("flags", &self.flags).finish()}
  }
  /// An event that occurred.
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct Event {
    /// User-provided value that got attached to `subscription::userdata`.
    pub userdata: Userdata,
    /// If non-zero, an error that occurred while processing the subscription request.
    pub error: Errno,
    /// The type of the event that occurred, and the contents of the event
    pub data: EventEnum,
  }
  impl core::fmt::Debug for Event {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("Event").field("userdata", &self.userdata).field("error", &self.error).field("data", &self.data).finish()}
  }
  /// The contents of an `event`.
  #[derive(Clone, Copy)]
  pub enum EventEnum{
    FdRead(EventFdReadwrite),
    FdWrite(EventFdReadwrite),
    Clock,
  }
  impl core::fmt::Debug for EventEnum {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        EventEnum::FdRead(e) => {
          f.debug_tuple("EventEnum::FdRead").field(e).finish()
        }
        EventEnum::FdWrite(e) => {
          f.debug_tuple("EventEnum::FdWrite").field(e).finish()
        }
        EventEnum::Clock => {
          f.debug_tuple("EventEnum::Clock").finish()
        }
      }
    }
  }
  /// The contents of a `subscription`, snapshot0 version.
  #[derive(Clone, Copy)]
  pub enum Snapshot0SubscriptionEnum{
    Clock(Snapshot0SubscriptionClock),
    Read(SubscriptionFsReadwrite),
    Write(SubscriptionFsReadwrite),
  }
  impl core::fmt::Debug for Snapshot0SubscriptionEnum {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        Snapshot0SubscriptionEnum::Clock(e) => {
          f.debug_tuple("Snapshot0SubscriptionEnum::Clock").field(e).finish()
        }
        Snapshot0SubscriptionEnum::Read(e) => {
          f.debug_tuple("Snapshot0SubscriptionEnum::Read").field(e).finish()
        }
        Snapshot0SubscriptionEnum::Write(e) => {
          f.debug_tuple("Snapshot0SubscriptionEnum::Write").field(e).finish()
        }
      }
    }
  }
  /// The contents of a `subscription`.
  #[derive(Clone, Copy)]
  pub enum SubscriptionEnum{
    Clock(SubscriptionClock),
    Read(SubscriptionFsReadwrite),
    Write(SubscriptionFsReadwrite),
  }
  impl core::fmt::Debug for SubscriptionEnum {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        SubscriptionEnum::Clock(e) => {
          f.debug_tuple("SubscriptionEnum::Clock").field(e).finish()
        }
        SubscriptionEnum::Read(e) => {
          f.debug_tuple("SubscriptionEnum::Read").field(e).finish()
        }
        SubscriptionEnum::Write(e) => {
          f.debug_tuple("SubscriptionEnum::Write").field(e).finish()
        }
      }
    }
  }
  /// The contents of a `subscription` when the variant is
  /// `eventtype::fd_read` or `eventtype::fd_write`.
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct SubscriptionFsReadwrite {
    /// The file descriptor on which to wait for it to become ready for reading or writing.
    pub file_descriptor: Fd,
  }
  impl core::fmt::Debug for SubscriptionFsReadwrite {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("SubscriptionFsReadwrite").field("file-descriptor", &self.file_descriptor).finish()}
  }
  impl wit_bindgen_wasmer::Endian for SubscriptionFsReadwrite {
    fn into_le(self) -> Self {
      Self {
        file_descriptor: self.file_descriptor.into_le(),
      }
    }
    fn from_le(self) -> Self {
      Self {
        file_descriptor: self.file_descriptor.from_le(),
      }
    }
  }
  unsafe impl wit_bindgen_wasmer::AllBytesValid for SubscriptionFsReadwrite {}
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct Snapshot0Subscription {
    pub userdata: Userdata,
    pub data: Snapshot0SubscriptionEnum,
  }
  impl core::fmt::Debug for Snapshot0Subscription {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("Snapshot0Subscription").field("userdata", &self.userdata).field("data", &self.data).finish()}
  }
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct Subscription {
    pub userdata: Userdata,
    pub data: SubscriptionEnum,
  }
  impl core::fmt::Debug for Subscription {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("Subscription").field("userdata", &self.userdata).field("data", &self.data).finish()}
  }
  
  /// Auxiliary data associated with the wasm exports.
  #[derive(Default)]
  pub struct WasiData {
  }
  
  pub struct Wasi {
    #[allow(dead_code)]
    env: wasmer::FunctionEnv<WasiData>,
    func_canonical_abi_realloc: wasmer::TypedFunction<(i32, i32, i32, i32), i32>,
    func_expose_types_dummy_func: wasmer::TypedFunction<i32, ()>,
    memory: wasmer::Memory,
  }
  impl Wasi {
    #[allow(unused_variables)]
    
    /// Adds any intrinsics, if necessary for this exported wasm
    /// functionality to the `ImportObject` provided.
    ///
    /// This function returns the `WasiData` which needs to be
    /// passed through to `Wasi::new`.
    fn add_to_imports(
    mut store: impl wasmer::AsStoreMut,
    imports: &mut wasmer::Imports,
    ) -> wasmer::FunctionEnv<WasiData> {
      let env = wasmer::FunctionEnv::new(&mut store, WasiData::default());
      env
    }
    
    /// Instantiates the provided `module` using the specified
    /// parameters, wrapping up the result in a structure that
    /// translates between wasm and the host.
    ///
    /// The `imports` provided will have intrinsics added to it
    /// automatically, so it's not necessary to call
    /// `add_to_imports` beforehand. This function will
    /// instantiate the `module` otherwise using `imports`, and
    /// both an instance of this structure and the underlying
    /// `wasmer::Instance` will be returned.
    pub fn instantiate(
    mut store: impl wasmer::AsStoreMut,
    module: &wasmer::Module,
    imports: &mut wasmer::Imports,
    ) -> anyhow::Result<(Self, wasmer::Instance)> {
      let env = Self::add_to_imports(&mut store, imports);
      let instance = wasmer::Instance::new(
      &mut store, module, &*imports)?;
      
      Ok((Self::new(store, &instance, env)?, instance))
    }
    
    /// Low-level creation wrapper for wrapping up the exports
    /// of the `instance` provided in this structure of wasm
    /// exports.
    ///
    /// This function will extract exports from the `instance`
    /// and wrap them all up in the returned structure which can
    /// be used to interact with the wasm module.
    pub fn new(
    store: impl wasmer::AsStoreMut,
    _instance: &wasmer::Instance,
    env: wasmer::FunctionEnv<WasiData>,
    ) -> Result<Self, wasmer::ExportError> {
      let func_canonical_abi_realloc= _instance.exports.get_typed_function(&store, "canonical_abi_realloc")?;
      let func_expose_types_dummy_func= _instance.exports.get_typed_function(&store, "expose-types-dummy-func")?;
      let memory= _instance.exports.get_memory("memory")?.clone();
      Ok(Wasi{
        func_canonical_abi_realloc,
        func_expose_types_dummy_func,
        memory,
        env,
      })
    }
    /// Dummy function to expose types into generated code
    pub fn expose_types_dummy_func(&self, store: &mut wasmer::Store,fd: Fd,dirent: Dirent,event_enum: EventEnum,event: Event,fdstat: Fdstat,subscription_clock: SubscriptionClock,snapshot0_subscription_clock: Snapshot0SubscriptionClock,subscription: Subscription,snapshot0_subscription: Snapshot0Subscription,)-> Result<(), wasmer::RuntimeError> {
      let func_canonical_abi_realloc = &self.func_canonical_abi_realloc;
      let _memory = &self.memory;
      let ptr0 = func_canonical_abi_realloc.call(store, 0, 0, 8, 296)?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 0, wit_bindgen_wasmer::rt::as_i32(wit_bindgen_wasmer::rt::as_i32(fd)))?;
      let Dirent{ d_next:d_next1, d_ino:d_ino1, d_type:d_type1, d_namlen:d_namlen1, } = dirent;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 8, wit_bindgen_wasmer::rt::as_i64(wit_bindgen_wasmer::rt::as_i64(d_next1)))?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 16, wit_bindgen_wasmer::rt::as_i64(wit_bindgen_wasmer::rt::as_i64(d_ino1)))?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 24, wit_bindgen_wasmer::rt::as_i32(d_type1 as i32) as u8)?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 28, wit_bindgen_wasmer::rt::as_i32(wit_bindgen_wasmer::rt::as_i32(d_namlen1)))?;
      match event_enum {
        EventEnum::FdRead(e) => {
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 32, wit_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
          let EventFdReadwrite{ nbytes:nbytes2, flags:flags2, } = e;
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 40, wit_bindgen_wasmer::rt::as_i64(wit_bindgen_wasmer::rt::as_i64(nbytes2)))?;
          let flags3 = flags2;
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 48, wit_bindgen_wasmer::rt::as_i32((flags3.bits >> 0) as i32) as u8)?;
        },
        EventEnum::FdWrite(e) => {
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 32, wit_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
          let EventFdReadwrite{ nbytes:nbytes4, flags:flags4, } = e;
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 40, wit_bindgen_wasmer::rt::as_i64(wit_bindgen_wasmer::rt::as_i64(nbytes4)))?;
          let flags5 = flags4;
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 48, wit_bindgen_wasmer::rt::as_i32((flags5.bits >> 0) as i32) as u8)?;
        },
        EventEnum::Clock=> {
          let e = ();
          {
            let _memory_view = _memory.view(&store);
            unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 32, wit_bindgen_wasmer::rt::as_i32(2i32) as u8)?;
            let () = e;
          }
        }
      };
      let Event{ userdata:userdata6, error:error6, data:data6, } = event;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 56, wit_bindgen_wasmer::rt::as_i64(wit_bindgen_wasmer::rt::as_i64(userdata6)))?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 64, wit_bindgen_wasmer::rt::as_i32(error6 as i32) as u8)?;
      match data6 {
        EventEnum::FdRead(e) => {
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 72, wit_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
          let EventFdReadwrite{ nbytes:nbytes7, flags:flags7, } = e;
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 80, wit_bindgen_wasmer::rt::as_i64(wit_bindgen_wasmer::rt::as_i64(nbytes7)))?;
          let flags8 = flags7;
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 88, wit_bindgen_wasmer::rt::as_i32((flags8.bits >> 0) as i32) as u8)?;
        },
        EventEnum::FdWrite(e) => {
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 72, wit_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
          let EventFdReadwrite{ nbytes:nbytes9, flags:flags9, } = e;
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 80, wit_bindgen_wasmer::rt::as_i64(wit_bindgen_wasmer::rt::as_i64(nbytes9)))?;
          let flags10 = flags9;
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 88, wit_bindgen_wasmer::rt::as_i32((flags10.bits >> 0) as i32) as u8)?;
        },
        EventEnum::Clock=> {
          let e = ();
          {
            let _memory_view = _memory.view(&store);
            unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 72, wit_bindgen_wasmer::rt::as_i32(2i32) as u8)?;
            let () = e;
          }
        }
      };
      let Fdstat{ fs_filetype:fs_filetype11, fs_flags:fs_flags11, fs_rights_base:fs_rights_base11, fs_rights_inheriting:fs_rights_inheriting11, } = fdstat;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 96, wit_bindgen_wasmer::rt::as_i32(fs_filetype11 as i32) as u8)?;
      let flags12 = fs_flags11;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 97, wit_bindgen_wasmer::rt::as_i32((flags12.bits >> 0) as i32) as u8)?;
      let flags13 = fs_rights_base11;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 104, wit_bindgen_wasmer::rt::as_i32((flags13.bits >> 32) as i32))?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 100, wit_bindgen_wasmer::rt::as_i32((flags13.bits >> 0) as i32))?;
      let flags14 = fs_rights_inheriting11;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 112, wit_bindgen_wasmer::rt::as_i32((flags14.bits >> 32) as i32))?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 108, wit_bindgen_wasmer::rt::as_i32((flags14.bits >> 0) as i32))?;
      let SubscriptionClock{ clock_id:clock_id15, timeout:timeout15, precision:precision15, flags:flags15, } = subscription_clock;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 120, wit_bindgen_wasmer::rt::as_i32(clock_id15 as i32) as u8)?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 128, wit_bindgen_wasmer::rt::as_i64(wit_bindgen_wasmer::rt::as_i64(timeout15)))?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 136, wit_bindgen_wasmer::rt::as_i64(wit_bindgen_wasmer::rt::as_i64(precision15)))?;
      let flags16 = flags15;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 144, wit_bindgen_wasmer::rt::as_i32((flags16.bits >> 0) as i32) as u8)?;
      let Snapshot0SubscriptionClock{ identifier:identifier17, id:id17, timeout:timeout17, precision:precision17, flags:flags17, } = snapshot0_subscription_clock;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 152, wit_bindgen_wasmer::rt::as_i64(wit_bindgen_wasmer::rt::as_i64(identifier17)))?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 160, wit_bindgen_wasmer::rt::as_i32(id17 as i32) as u8)?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 168, wit_bindgen_wasmer::rt::as_i64(wit_bindgen_wasmer::rt::as_i64(timeout17)))?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 176, wit_bindgen_wasmer::rt::as_i64(wit_bindgen_wasmer::rt::as_i64(precision17)))?;
      let flags18 = flags17;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 184, wit_bindgen_wasmer::rt::as_i32((flags18.bits >> 0) as i32) as u8)?;
      let Subscription{ userdata:userdata19, data:data19, } = subscription;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 192, wit_bindgen_wasmer::rt::as_i64(wit_bindgen_wasmer::rt::as_i64(userdata19)))?;
      match data19 {
        SubscriptionEnum::Clock(e) => {
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 200, wit_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
          let SubscriptionClock{ clock_id:clock_id20, timeout:timeout20, precision:precision20, flags:flags20, } = e;
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 208, wit_bindgen_wasmer::rt::as_i32(clock_id20 as i32) as u8)?;
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 216, wit_bindgen_wasmer::rt::as_i64(wit_bindgen_wasmer::rt::as_i64(timeout20)))?;
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 224, wit_bindgen_wasmer::rt::as_i64(wit_bindgen_wasmer::rt::as_i64(precision20)))?;
          let flags21 = flags20;
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 232, wit_bindgen_wasmer::rt::as_i32((flags21.bits >> 0) as i32) as u8)?;
        },
        SubscriptionEnum::Read(e) => {
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 200, wit_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
          let SubscriptionFsReadwrite{ file_descriptor:file_descriptor22, } = e;
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 208, wit_bindgen_wasmer::rt::as_i32(wit_bindgen_wasmer::rt::as_i32(file_descriptor22)))?;
        },
        SubscriptionEnum::Write(e) => {
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 200, wit_bindgen_wasmer::rt::as_i32(2i32) as u8)?;
          let SubscriptionFsReadwrite{ file_descriptor:file_descriptor23, } = e;
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 208, wit_bindgen_wasmer::rt::as_i32(wit_bindgen_wasmer::rt::as_i32(file_descriptor23)))?;
        },
      };
      let Snapshot0Subscription{ userdata:userdata24, data:data24, } = snapshot0_subscription;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 240, wit_bindgen_wasmer::rt::as_i64(wit_bindgen_wasmer::rt::as_i64(userdata24)))?;
      match data24 {
        Snapshot0SubscriptionEnum::Clock(e) => {
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 248, wit_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
          let Snapshot0SubscriptionClock{ identifier:identifier25, id:id25, timeout:timeout25, precision:precision25, flags:flags25, } = e;
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 256, wit_bindgen_wasmer::rt::as_i64(wit_bindgen_wasmer::rt::as_i64(identifier25)))?;
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 264, wit_bindgen_wasmer::rt::as_i32(id25 as i32) as u8)?;
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 272, wit_bindgen_wasmer::rt::as_i64(wit_bindgen_wasmer::rt::as_i64(timeout25)))?;
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 280, wit_bindgen_wasmer::rt::as_i64(wit_bindgen_wasmer::rt::as_i64(precision25)))?;
          let flags26 = flags25;
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 288, wit_bindgen_wasmer::rt::as_i32((flags26.bits >> 0) as i32) as u8)?;
        },
        Snapshot0SubscriptionEnum::Read(e) => {
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 248, wit_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
          let SubscriptionFsReadwrite{ file_descriptor:file_descriptor27, } = e;
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 256, wit_bindgen_wasmer::rt::as_i32(wit_bindgen_wasmer::rt::as_i32(file_descriptor27)))?;
        },
        Snapshot0SubscriptionEnum::Write(e) => {
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 248, wit_bindgen_wasmer::rt::as_i32(2i32) as u8)?;
          let SubscriptionFsReadwrite{ file_descriptor:file_descriptor28, } = e;
          let _memory_view = _memory.view(&store);
          unsafe { _memory_view.data_unchecked_mut() }.store(ptr0 + 256, wit_bindgen_wasmer::rt::as_i32(wit_bindgen_wasmer::rt::as_i32(file_descriptor28)))?;
        },
      };
      self.func_expose_types_dummy_func.call(store, ptr0, )?;
      Ok(())
    }
  }
  #[allow(unused_imports)]
  use wasmer::AsStoreMut as _;
  #[allow(unused_imports)]
  use wasmer::AsStoreRef as _;
  use wit_bindgen_wasmer::rt::RawMem;
}
