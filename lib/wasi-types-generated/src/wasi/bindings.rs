#[allow(clippy::all)]
pub mod output {
  /// Type names used by low-level WASI interfaces.
  /// An array size.
  /// 
  /// Note: This is similar to `size_t` in POSIX.
  pub type Size = u32;
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
  /// Identifier for a device containing a file system. Can be used in combination
  /// with `inode` to uniquely identify a file or directory in the filesystem.
  pub type Device = u64;
  pub type Linkcount = u64;
  pub type Snapshot0Linkcount = u32;
  pub type Tid = u32;
  pub type Pid = u32;
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
  wit_bindgen_rust::bitflags::bitflags! {
    /// File descriptor rights, determining which actions may be performed.
    pub struct Rights: u64 {
      /// The right to invoke `fd_datasync`.
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
  impl Rights {
        /// Convert from a raw integer, preserving any unknown bits. See
        /// <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>
        pub fn from_bits_preserve(bits: u64) -> Self {
              Self { bits }
        }
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
  /// A directory entry, snapshot0 version.
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct Snapshot0Dirent {
    /// The offset of the next directory entry stored in this directory.
    pub d_next: Dircookie,
    /// The serial number of the file referred to by this directory entry.
    pub d_ino: Inode,
    /// The length of the name of the directory entry.
    pub d_namlen: Dirnamlen,
    /// The type of the file referred to by this directory entry.
    pub d_type: Filetype,
  }
  impl core::fmt::Debug for Snapshot0Dirent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("Snapshot0Dirent").field("d-next", &self.d_next).field("d-ino", &self.d_ino).field("d-namlen", &self.d_namlen).field("d-type", &self.d_type).finish()}
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
  wit_bindgen_rust::bitflags::bitflags! {
    /// File descriptor flags.
    pub struct Fdflags: u8 {
      /// Append mode: Data written to the file is always appended to the file's end.
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
  impl Fdflags {
        /// Convert from a raw integer, preserving any unknown bits. See
        /// <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>
        pub fn from_bits_preserve(bits: u8) -> Self {
              Self { bits }
        }
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
  wit_bindgen_rust::bitflags::bitflags! {
    /// Which file time attributes to adjust.
    /// TODO: wit appears to not have support for flags repr
    /// (@witx repr u16)
    pub struct Fstflags: u8 {
      /// Adjust the last data access timestamp to the value stored in `filestat::atim`.
      const SET_ATIM = 1 << 0;
      /// Adjust the last data access timestamp to the time of clock `clockid::realtime`.
      const SET_ATIM_NOW = 1 << 1;
      /// Adjust the last data modification timestamp to the value stored in `filestat::mtim`.
      const SET_MTIM = 1 << 2;
      /// Adjust the last data modification timestamp to the time of clock `clockid::realtime`.
      const SET_MTIM_NOW = 1 << 3;
    }
  }
  impl Fstflags {
        /// Convert from a raw integer, preserving any unknown bits. See
        /// <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>
        pub fn from_bits_preserve(bits: u8) -> Self {
              Self { bits }
        }
  }
  wit_bindgen_rust::bitflags::bitflags! {
    /// Flags determining the method of how paths are resolved.
    /// TODO: wit appears to not have support for flags repr
    /// (@witx repr u32)
    pub struct Lookup: u8 {
      /// As long as the resolved path corresponds to a symbolic link, it is expanded.
      const SYMLINK_FOLLOW = 1 << 0;
    }
  }
  impl Lookup {
        /// Convert from a raw integer, preserving any unknown bits. See
        /// <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>
        pub fn from_bits_preserve(bits: u8) -> Self {
              Self { bits }
        }
  }
  wit_bindgen_rust::bitflags::bitflags! {
    /// Open flags used by `path_open`.
    /// TODO: wit appears to not have support for flags repr
    /// (@witx repr u16)
    pub struct Oflags: u8 {
      /// Create file if it does not exist.
      const CREATE = 1 << 0;
      /// Fail if not a directory.
      const DIRECTORY = 1 << 1;
      /// Fail if file already exists.
      const EXCL = 1 << 2;
      /// Truncate file to size 0.
      const TRUNC = 1 << 3;
    }
  }
  impl Oflags {
        /// Convert from a raw integer, preserving any unknown bits. See
        /// <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>
        pub fn from_bits_preserve(bits: u8) -> Self {
              Self { bits }
        }
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
  wit_bindgen_rust::bitflags::bitflags! {
    /// Flags determining how to interpret the timestamp provided in
    /// `subscription-clock::timeout`.
    pub struct Subclockflags: u8 {
      /// If set, treat the timestamp provided in
      /// `subscription-clock::timeout` as an absolute timestamp of clock
      /// `subscription-clock::id`. If clear, treat the timestamp
      /// provided in `subscription-clock::timeout` relative to the
      /// current time value of clock `subscription-clock::id`.
      const SUBSCRIPTION_CLOCK_ABSTIME = 1 << 0;
    }
  }
  impl Subclockflags {
        /// Convert from a raw integer, preserving any unknown bits. See
        /// <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>
        pub fn from_bits_preserve(bits: u8) -> Self {
              Self { bits }
        }
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
  wit_bindgen_rust::bitflags::bitflags! {
    /// The state of the file descriptor subscribed to with
    /// `eventtype::fd_read` or `eventtype::fd_write`.
    pub struct Eventrwflags: u8 {
      /// The peer of this socket has closed or disconnected.
      const FD_READWRITE_HANGUP = 1 << 0;
    }
  }
  impl Eventrwflags {
        /// Convert from a raw integer, preserving any unknown bits. See
        /// <https://github.com/bitflags/bitflags/issues/263#issuecomment-957088321>
        pub fn from_bits_preserve(bits: u8) -> Self {
              Self { bits }
        }
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
  /// An event that occurred.
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct Snapshot0Event {
    /// User-provided value that got attached to `subscription::userdata`.
    pub userdata: Userdata,
    /// If non-zero, an error that occurred while processing the subscription request.
    pub error: Errno,
    /// The type of event that occured
    pub type_: Eventtype,
    /// The contents of the event, if it is an `eventtype::fd_read` or
    /// `eventtype::fd_write`. `eventtype::clock` events ignore this field.
    pub fd_readwrite: EventFdReadwrite,
  }
  impl core::fmt::Debug for Snapshot0Event {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("Snapshot0Event").field("userdata", &self.userdata).field("error", &self.error).field("type", &self.type_).field("fd-readwrite", &self.fd_readwrite).finish()}
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
  #[repr(u8)]
  #[derive(Clone, Copy, PartialEq, Eq)]
  pub enum Socktype {
    Dgram,
    Stream,
    Raw,
    Seqpacket,
  }
  impl core::fmt::Debug for Socktype {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        Socktype::Dgram => {
          f.debug_tuple("Socktype::Dgram").finish()
        }
        Socktype::Stream => {
          f.debug_tuple("Socktype::Stream").finish()
        }
        Socktype::Raw => {
          f.debug_tuple("Socktype::Raw").finish()
        }
        Socktype::Seqpacket => {
          f.debug_tuple("Socktype::Seqpacket").finish()
        }
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
  }
  impl core::fmt::Debug for Sockstatus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        Sockstatus::Opening => {
          f.debug_tuple("Sockstatus::Opening").finish()
        }
        Sockstatus::Opened => {
          f.debug_tuple("Sockstatus::Opened").finish()
        }
        Sockstatus::Closed => {
          f.debug_tuple("Sockstatus::Closed").finish()
        }
        Sockstatus::Failed => {
          f.debug_tuple("Sockstatus::Failed").finish()
        }
      }
    }
  }
  #[repr(u8)]
  #[derive(Clone, Copy, PartialEq, Eq)]
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
        Sockoption::Noop => {
          f.debug_tuple("Sockoption::Noop").finish()
        }
        Sockoption::ReusePort => {
          f.debug_tuple("Sockoption::ReusePort").finish()
        }
        Sockoption::ReuseAddr => {
          f.debug_tuple("Sockoption::ReuseAddr").finish()
        }
        Sockoption::NoDelay => {
          f.debug_tuple("Sockoption::NoDelay").finish()
        }
        Sockoption::DontRoute => {
          f.debug_tuple("Sockoption::DontRoute").finish()
        }
        Sockoption::OnlyV6 => {
          f.debug_tuple("Sockoption::OnlyV6").finish()
        }
        Sockoption::Broadcast => {
          f.debug_tuple("Sockoption::Broadcast").finish()
        }
        Sockoption::MulticastLoopV4 => {
          f.debug_tuple("Sockoption::MulticastLoopV4").finish()
        }
        Sockoption::MulticastLoopV6 => {
          f.debug_tuple("Sockoption::MulticastLoopV6").finish()
        }
        Sockoption::Promiscuous => {
          f.debug_tuple("Sockoption::Promiscuous").finish()
        }
        Sockoption::Listening => {
          f.debug_tuple("Sockoption::Listening").finish()
        }
        Sockoption::LastError => {
          f.debug_tuple("Sockoption::LastError").finish()
        }
        Sockoption::KeepAlive => {
          f.debug_tuple("Sockoption::KeepAlive").finish()
        }
        Sockoption::Linger => {
          f.debug_tuple("Sockoption::Linger").finish()
        }
        Sockoption::OobInline => {
          f.debug_tuple("Sockoption::OobInline").finish()
        }
        Sockoption::RecvBufSize => {
          f.debug_tuple("Sockoption::RecvBufSize").finish()
        }
        Sockoption::SendBufSize => {
          f.debug_tuple("Sockoption::SendBufSize").finish()
        }
        Sockoption::RecvLowat => {
          f.debug_tuple("Sockoption::RecvLowat").finish()
        }
        Sockoption::SendLowat => {
          f.debug_tuple("Sockoption::SendLowat").finish()
        }
        Sockoption::RecvTimeout => {
          f.debug_tuple("Sockoption::RecvTimeout").finish()
        }
        Sockoption::SendTimeout => {
          f.debug_tuple("Sockoption::SendTimeout").finish()
        }
        Sockoption::ConnectTimeout => {
          f.debug_tuple("Sockoption::ConnectTimeout").finish()
        }
        Sockoption::AcceptTimeout => {
          f.debug_tuple("Sockoption::AcceptTimeout").finish()
        }
        Sockoption::Ttl => {
          f.debug_tuple("Sockoption::Ttl").finish()
        }
        Sockoption::MulticastTtlV4 => {
          f.debug_tuple("Sockoption::MulticastTtlV4").finish()
        }
        Sockoption::Type => {
          f.debug_tuple("Sockoption::Type").finish()
        }
        Sockoption::Proto => {
          f.debug_tuple("Sockoption::Proto").finish()
        }
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
  }
  impl core::fmt::Debug for Streamsecurity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        Streamsecurity::Unencrypted => {
          f.debug_tuple("Streamsecurity::Unencrypted").finish()
        }
        Streamsecurity::AnyEncryption => {
          f.debug_tuple("Streamsecurity::AnyEncryption").finish()
        }
        Streamsecurity::ClassicEncryption => {
          f.debug_tuple("Streamsecurity::ClassicEncryption").finish()
        }
        Streamsecurity::DoubleEncryption => {
          f.debug_tuple("Streamsecurity::DoubleEncryption").finish()
        }
      }
    }
  }
  #[repr(u8)]
  #[derive(Clone, Copy, PartialEq, Eq)]
  pub enum Addressfamily {
    Unspec,
    Inet4,
    Inet6,
    Unix,
  }
  impl core::fmt::Debug for Addressfamily {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        Addressfamily::Unspec => {
          f.debug_tuple("Addressfamily::Unspec").finish()
        }
        Addressfamily::Inet4 => {
          f.debug_tuple("Addressfamily::Inet4").finish()
        }
        Addressfamily::Inet6 => {
          f.debug_tuple("Addressfamily::Inet6").finish()
        }
        Addressfamily::Unix => {
          f.debug_tuple("Addressfamily::Unix").finish()
        }
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
      f.debug_struct("Snapshot0Filestat").field("st-dev", &self.st_dev).field("st-ino", &self.st_ino).field("st-filetype", &self.st_filetype).field("st-nlink", &self.st_nlink).field("st-size", &self.st_size).field("st-atim", &self.st_atim).field("st-mtim", &self.st_mtim).field("st-ctim", &self.st_ctim).finish()}
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
      f.debug_struct("Filestat").field("st-dev", &self.st_dev).field("st-ino", &self.st_ino).field("st-filetype", &self.st_filetype).field("st-nlink", &self.st_nlink).field("st-size", &self.st_size).field("st-atim", &self.st_atim).field("st-mtim", &self.st_mtim).field("st-ctim", &self.st_ctim).finish()}
  }
  #[repr(u8)]
  #[derive(Clone, Copy, PartialEq, Eq)]
  pub enum Snapshot0Whence {
    Cur,
    End,
    Set,
  }
  impl core::fmt::Debug for Snapshot0Whence {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        Snapshot0Whence::Cur => {
          f.debug_tuple("Snapshot0Whence::Cur").finish()
        }
        Snapshot0Whence::End => {
          f.debug_tuple("Snapshot0Whence::End").finish()
        }
        Snapshot0Whence::Set => {
          f.debug_tuple("Snapshot0Whence::Set").finish()
        }
      }
    }
  }
  #[repr(u8)]
  #[derive(Clone, Copy, PartialEq, Eq)]
  pub enum Whence {
    Set,
    Cur,
    End,
  }
  impl core::fmt::Debug for Whence {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        Whence::Set => {
          f.debug_tuple("Whence::Set").finish()
        }
        Whence::Cur => {
          f.debug_tuple("Whence::Cur").finish()
        }
        Whence::End => {
          f.debug_tuple("Whence::End").finish()
        }
      }
    }
  }
  #[repr(C)]
  #[derive(Copy, Clone)]
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
      f.debug_struct("Tty").field("cols", &self.cols).field("rows", &self.rows).field("width", &self.width).field("height", &self.height).field("stdin-tty", &self.stdin_tty).field("stdout-tty", &self.stdout_tty).field("stderr-tty", &self.stderr_tty).field("echo", &self.echo).field("line-buffered", &self.line_buffered).finish()}
  }
  #[repr(u8)]
  #[derive(Clone, Copy, PartialEq, Eq)]
  pub enum BusDataFormat {
    Raw,
    Bincode,
    MessagePack,
    Json,
    Yaml,
    Xml,
    Rkyv,
  }
  impl core::fmt::Debug for BusDataFormat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        BusDataFormat::Raw => {
          f.debug_tuple("BusDataFormat::Raw").finish()
        }
        BusDataFormat::Bincode => {
          f.debug_tuple("BusDataFormat::Bincode").finish()
        }
        BusDataFormat::MessagePack => {
          f.debug_tuple("BusDataFormat::MessagePack").finish()
        }
        BusDataFormat::Json => {
          f.debug_tuple("BusDataFormat::Json").finish()
        }
        BusDataFormat::Yaml => {
          f.debug_tuple("BusDataFormat::Yaml").finish()
        }
        BusDataFormat::Xml => {
          f.debug_tuple("BusDataFormat::Xml").finish()
        }
        BusDataFormat::Rkyv => {
          f.debug_tuple("BusDataFormat::Rkyv").finish()
        }
      }
    }
  }
  #[repr(u8)]
  #[derive(Clone, Copy, PartialEq, Eq)]
  pub enum BusEventType {
    Noop,
    Exit,
    Call,
    Result,
    Fault,
    Close,
  }
  impl core::fmt::Debug for BusEventType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        BusEventType::Noop => {
          f.debug_tuple("BusEventType::Noop").finish()
        }
        BusEventType::Exit => {
          f.debug_tuple("BusEventType::Exit").finish()
        }
        BusEventType::Call => {
          f.debug_tuple("BusEventType::Call").finish()
        }
        BusEventType::Result => {
          f.debug_tuple("BusEventType::Result").finish()
        }
        BusEventType::Fault => {
          f.debug_tuple("BusEventType::Fault").finish()
        }
        BusEventType::Close => {
          f.debug_tuple("BusEventType::Close").finish()
        }
      }
    }
  }
  pub type Bid = u32;
  pub type Cid = u32;
  /// __wasi_option_t
  #[repr(u8)]
  #[derive(Clone, Copy, PartialEq, Eq)]
  pub enum OptionTag {
    None,
    Some,
  }
  impl core::fmt::Debug for OptionTag {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        OptionTag::None => {
          f.debug_tuple("OptionTag::None").finish()
        }
        OptionTag::Some => {
          f.debug_tuple("OptionTag::Some").finish()
        }
      }
    }
  }
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct OptionBid {
    pub tag: OptionTag,
    pub bid: Bid,
  }
  impl core::fmt::Debug for OptionBid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("OptionBid").field("tag", &self.tag).field("bid", &self.bid).finish()}
  }
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct OptionCid {
    pub tag: OptionTag,
    pub cid: Cid,
  }
  impl core::fmt::Debug for OptionCid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("OptionCid").field("tag", &self.tag).field("cid", &self.cid).finish()}
  }
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct OptionFd {
    pub tag: OptionTag,
    pub fd: Fd,
  }
  impl core::fmt::Debug for OptionFd {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("OptionFd").field("tag", &self.tag).field("fd", &self.fd).finish()}
  }
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct BusHandles {
    pub bid: Bid,
    pub stdin: OptionFd,
    pub stdout: OptionFd,
    pub stderr: OptionFd,
  }
  impl core::fmt::Debug for BusHandles {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("BusHandles").field("bid", &self.bid).field("stdin", &self.stdin).field("stdout", &self.stdout).field("stderr", &self.stderr).finish()}
  }
  pub type ExitCode = u32;
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct BusEventExit {
    pub bid: Bid,
    pub rval: ExitCode,
  }
  impl core::fmt::Debug for BusEventExit {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("BusEventExit").field("bid", &self.bid).field("rval", &self.rval).finish()}
  }
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct BusEventFault {
    pub cid: Cid,
    pub err: BusErrno,
  }
  impl core::fmt::Debug for BusEventFault {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("BusEventFault").field("cid", &self.cid).field("err", &self.err).finish()}
  }
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct BusEventClose {
    pub cid: Cid,
  }
  impl core::fmt::Debug for BusEventClose {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("BusEventClose").field("cid", &self.cid).finish()}
  }
  pub type EventFdFlags = u16;
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct PrestatUDir {
    pub pr_name_len: u32,
  }
  impl core::fmt::Debug for PrestatUDir {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("PrestatUDir").field("pr-name-len", &self.pr_name_len).finish()}
  }
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct PrestatU {
    pub dir: PrestatUDir,
  }
  impl core::fmt::Debug for PrestatU {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("PrestatU").field("dir", &self.dir).finish()}
  }
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct Prestat {
    pub pr_type: Preopentype,
    pub u: PrestatU,
  }
  impl core::fmt::Debug for Prestat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("Prestat").field("pr-type", &self.pr_type).field("u", &self.u).finish()}
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
      f.debug_struct("PipeHandles").field("pipe", &self.pipe).field("other", &self.other).finish()}
  }
  #[repr(u8)]
  #[derive(Clone, Copy, PartialEq, Eq)]
  pub enum StdioMode {
    Reserved,
    Piped,
    Inherit,
    Null,
    Log,
  }
  impl core::fmt::Debug for StdioMode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        StdioMode::Reserved => {
          f.debug_tuple("StdioMode::Reserved").finish()
        }
        StdioMode::Piped => {
          f.debug_tuple("StdioMode::Piped").finish()
        }
        StdioMode::Inherit => {
          f.debug_tuple("StdioMode::Inherit").finish()
        }
        StdioMode::Null => {
          f.debug_tuple("StdioMode::Null").finish()
        }
        StdioMode::Log => {
          f.debug_tuple("StdioMode::Log").finish()
        }
      }
    }
  }
  #[repr(u16)]
  #[derive(Clone, Copy, PartialEq, Eq)]
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
        SockProto::Ip => {
          f.debug_tuple("SockProto::Ip").finish()
        }
        SockProto::Icmp => {
          f.debug_tuple("SockProto::Icmp").finish()
        }
        SockProto::Igmp => {
          f.debug_tuple("SockProto::Igmp").finish()
        }
        SockProto::ProtoThree => {
          f.debug_tuple("SockProto::ProtoThree").finish()
        }
        SockProto::Ipip => {
          f.debug_tuple("SockProto::Ipip").finish()
        }
        SockProto::ProtoFive => {
          f.debug_tuple("SockProto::ProtoFive").finish()
        }
        SockProto::Tcp => {
          f.debug_tuple("SockProto::Tcp").finish()
        }
        SockProto::ProtoSeven => {
          f.debug_tuple("SockProto::ProtoSeven").finish()
        }
        SockProto::Egp => {
          f.debug_tuple("SockProto::Egp").finish()
        }
        SockProto::ProtoNine => {
          f.debug_tuple("SockProto::ProtoNine").finish()
        }
        SockProto::ProtoTen => {
          f.debug_tuple("SockProto::ProtoTen").finish()
        }
        SockProto::ProtoEleven => {
          f.debug_tuple("SockProto::ProtoEleven").finish()
        }
        SockProto::Pup => {
          f.debug_tuple("SockProto::Pup").finish()
        }
        SockProto::ProtoThirteen => {
          f.debug_tuple("SockProto::ProtoThirteen").finish()
        }
        SockProto::ProtoFourteen => {
          f.debug_tuple("SockProto::ProtoFourteen").finish()
        }
        SockProto::ProtoFifteen => {
          f.debug_tuple("SockProto::ProtoFifteen").finish()
        }
        SockProto::ProtoSixteen => {
          f.debug_tuple("SockProto::ProtoSixteen").finish()
        }
        SockProto::Udp => {
          f.debug_tuple("SockProto::Udp").finish()
        }
        SockProto::ProtoEighteen => {
          f.debug_tuple("SockProto::ProtoEighteen").finish()
        }
        SockProto::ProtoNineteen => {
          f.debug_tuple("SockProto::ProtoNineteen").finish()
        }
        SockProto::ProtoTwenty => {
          f.debug_tuple("SockProto::ProtoTwenty").finish()
        }
        SockProto::ProtoTwentyone => {
          f.debug_tuple("SockProto::ProtoTwentyone").finish()
        }
        SockProto::Idp => {
          f.debug_tuple("SockProto::Idp").finish()
        }
        SockProto::ProtoTwentythree => {
          f.debug_tuple("SockProto::ProtoTwentythree").finish()
        }
        SockProto::ProtoTwentyfour => {
          f.debug_tuple("SockProto::ProtoTwentyfour").finish()
        }
        SockProto::ProtoTwentyfive => {
          f.debug_tuple("SockProto::ProtoTwentyfive").finish()
        }
        SockProto::ProtoTwentysix => {
          f.debug_tuple("SockProto::ProtoTwentysix").finish()
        }
        SockProto::ProtoTwentyseven => {
          f.debug_tuple("SockProto::ProtoTwentyseven").finish()
        }
        SockProto::ProtoTwentyeight => {
          f.debug_tuple("SockProto::ProtoTwentyeight").finish()
        }
        SockProto::ProtoTp => {
          f.debug_tuple("SockProto::ProtoTp").finish()
        }
        SockProto::ProtoThirty => {
          f.debug_tuple("SockProto::ProtoThirty").finish()
        }
        SockProto::ProtoThirtyone => {
          f.debug_tuple("SockProto::ProtoThirtyone").finish()
        }
        SockProto::ProtoThirtytwo => {
          f.debug_tuple("SockProto::ProtoThirtytwo").finish()
        }
        SockProto::Dccp => {
          f.debug_tuple("SockProto::Dccp").finish()
        }
        SockProto::ProtoThirtyfour => {
          f.debug_tuple("SockProto::ProtoThirtyfour").finish()
        }
        SockProto::ProtoThirtyfive => {
          f.debug_tuple("SockProto::ProtoThirtyfive").finish()
        }
        SockProto::ProtoThirtysix => {
          f.debug_tuple("SockProto::ProtoThirtysix").finish()
        }
        SockProto::ProtoThirtyseven => {
          f.debug_tuple("SockProto::ProtoThirtyseven").finish()
        }
        SockProto::ProtoThirtyeight => {
          f.debug_tuple("SockProto::ProtoThirtyeight").finish()
        }
        SockProto::ProtoThirtynine => {
          f.debug_tuple("SockProto::ProtoThirtynine").finish()
        }
        SockProto::ProtoFourty => {
          f.debug_tuple("SockProto::ProtoFourty").finish()
        }
        SockProto::Ipv6 => {
          f.debug_tuple("SockProto::Ipv6").finish()
        }
        SockProto::ProtoFourtytwo => {
          f.debug_tuple("SockProto::ProtoFourtytwo").finish()
        }
        SockProto::Routing => {
          f.debug_tuple("SockProto::Routing").finish()
        }
        SockProto::Fragment => {
          f.debug_tuple("SockProto::Fragment").finish()
        }
        SockProto::ProtoFourtyfive => {
          f.debug_tuple("SockProto::ProtoFourtyfive").finish()
        }
        SockProto::Rsvp => {
          f.debug_tuple("SockProto::Rsvp").finish()
        }
        SockProto::Gre => {
          f.debug_tuple("SockProto::Gre").finish()
        }
        SockProto::ProtoFourtyeight => {
          f.debug_tuple("SockProto::ProtoFourtyeight").finish()
        }
        SockProto::ProtoFourtynine => {
          f.debug_tuple("SockProto::ProtoFourtynine").finish()
        }
        SockProto::Esp => {
          f.debug_tuple("SockProto::Esp").finish()
        }
        SockProto::Ah => {
          f.debug_tuple("SockProto::Ah").finish()
        }
        SockProto::ProtoFiftytwo => {
          f.debug_tuple("SockProto::ProtoFiftytwo").finish()
        }
        SockProto::ProtoFiftythree => {
          f.debug_tuple("SockProto::ProtoFiftythree").finish()
        }
        SockProto::ProtoFiftyfour => {
          f.debug_tuple("SockProto::ProtoFiftyfour").finish()
        }
        SockProto::ProtoFiftyfive => {
          f.debug_tuple("SockProto::ProtoFiftyfive").finish()
        }
        SockProto::ProtoFiftysix => {
          f.debug_tuple("SockProto::ProtoFiftysix").finish()
        }
        SockProto::ProtoFiftyseven => {
          f.debug_tuple("SockProto::ProtoFiftyseven").finish()
        }
        SockProto::Icmpv6 => {
          f.debug_tuple("SockProto::Icmpv6").finish()
        }
        SockProto::None => {
          f.debug_tuple("SockProto::None").finish()
        }
        SockProto::Dstopts => {
          f.debug_tuple("SockProto::Dstopts").finish()
        }
        SockProto::ProtoSixtyone => {
          f.debug_tuple("SockProto::ProtoSixtyone").finish()
        }
        SockProto::ProtoSixtytwo => {
          f.debug_tuple("SockProto::ProtoSixtytwo").finish()
        }
        SockProto::ProtoSixtythree => {
          f.debug_tuple("SockProto::ProtoSixtythree").finish()
        }
        SockProto::ProtoSixtyfour => {
          f.debug_tuple("SockProto::ProtoSixtyfour").finish()
        }
        SockProto::ProtoSixtyfive => {
          f.debug_tuple("SockProto::ProtoSixtyfive").finish()
        }
        SockProto::ProtoSixtysix => {
          f.debug_tuple("SockProto::ProtoSixtysix").finish()
        }
        SockProto::ProtoSixtyseven => {
          f.debug_tuple("SockProto::ProtoSixtyseven").finish()
        }
        SockProto::ProtoSixtyeight => {
          f.debug_tuple("SockProto::ProtoSixtyeight").finish()
        }
        SockProto::ProtoSixtynine => {
          f.debug_tuple("SockProto::ProtoSixtynine").finish()
        }
        SockProto::ProtoSeventy => {
          f.debug_tuple("SockProto::ProtoSeventy").finish()
        }
        SockProto::ProtoSeventyone => {
          f.debug_tuple("SockProto::ProtoSeventyone").finish()
        }
        SockProto::ProtoSeventytwo => {
          f.debug_tuple("SockProto::ProtoSeventytwo").finish()
        }
        SockProto::ProtoSeventythree => {
          f.debug_tuple("SockProto::ProtoSeventythree").finish()
        }
        SockProto::ProtoSeventyfour => {
          f.debug_tuple("SockProto::ProtoSeventyfour").finish()
        }
        SockProto::ProtoSeventyfive => {
          f.debug_tuple("SockProto::ProtoSeventyfive").finish()
        }
        SockProto::ProtoSeventysix => {
          f.debug_tuple("SockProto::ProtoSeventysix").finish()
        }
        SockProto::ProtoSeventyseven => {
          f.debug_tuple("SockProto::ProtoSeventyseven").finish()
        }
        SockProto::ProtoSeventyeight => {
          f.debug_tuple("SockProto::ProtoSeventyeight").finish()
        }
        SockProto::ProtoSeventynine => {
          f.debug_tuple("SockProto::ProtoSeventynine").finish()
        }
        SockProto::ProtoEighty => {
          f.debug_tuple("SockProto::ProtoEighty").finish()
        }
        SockProto::ProtoEightyone => {
          f.debug_tuple("SockProto::ProtoEightyone").finish()
        }
        SockProto::ProtoEightytwo => {
          f.debug_tuple("SockProto::ProtoEightytwo").finish()
        }
        SockProto::ProtoEightythree => {
          f.debug_tuple("SockProto::ProtoEightythree").finish()
        }
        SockProto::ProtoEightyfour => {
          f.debug_tuple("SockProto::ProtoEightyfour").finish()
        }
        SockProto::ProtoEightyfive => {
          f.debug_tuple("SockProto::ProtoEightyfive").finish()
        }
        SockProto::ProtoEightysix => {
          f.debug_tuple("SockProto::ProtoEightysix").finish()
        }
        SockProto::ProtoEightyseven => {
          f.debug_tuple("SockProto::ProtoEightyseven").finish()
        }
        SockProto::ProtoEightyeight => {
          f.debug_tuple("SockProto::ProtoEightyeight").finish()
        }
        SockProto::ProtoEightynine => {
          f.debug_tuple("SockProto::ProtoEightynine").finish()
        }
        SockProto::ProtoNinety => {
          f.debug_tuple("SockProto::ProtoNinety").finish()
        }
        SockProto::ProtoNinetyone => {
          f.debug_tuple("SockProto::ProtoNinetyone").finish()
        }
        SockProto::Mtp => {
          f.debug_tuple("SockProto::Mtp").finish()
        }
        SockProto::ProtoNinetythree => {
          f.debug_tuple("SockProto::ProtoNinetythree").finish()
        }
        SockProto::Beetph => {
          f.debug_tuple("SockProto::Beetph").finish()
        }
        SockProto::ProtoNinetyfive => {
          f.debug_tuple("SockProto::ProtoNinetyfive").finish()
        }
        SockProto::ProtoNinetysix => {
          f.debug_tuple("SockProto::ProtoNinetysix").finish()
        }
        SockProto::ProtoNineetyseven => {
          f.debug_tuple("SockProto::ProtoNineetyseven").finish()
        }
        SockProto::Encap => {
          f.debug_tuple("SockProto::Encap").finish()
        }
        SockProto::ProtoNinetynine => {
          f.debug_tuple("SockProto::ProtoNinetynine").finish()
        }
        SockProto::ProtoOnehundred => {
          f.debug_tuple("SockProto::ProtoOnehundred").finish()
        }
        SockProto::ProtoOnehundredandone => {
          f.debug_tuple("SockProto::ProtoOnehundredandone").finish()
        }
        SockProto::ProtoOnehundredandtwo => {
          f.debug_tuple("SockProto::ProtoOnehundredandtwo").finish()
        }
        SockProto::Pim => {
          f.debug_tuple("SockProto::Pim").finish()
        }
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
        SockProto::Comp => {
          f.debug_tuple("SockProto::Comp").finish()
        }
        SockProto::ProtoOnehundredandnine => {
          f.debug_tuple("SockProto::ProtoOnehundredandnine").finish()
        }
        SockProto::ProtoOnehundredandten => {
          f.debug_tuple("SockProto::ProtoOnehundredandten").finish()
        }
        SockProto::ProtoOnehundredandeleven => {
          f.debug_tuple("SockProto::ProtoOnehundredandeleven").finish()
        }
        SockProto::ProtoOnehundredandtwelve => {
          f.debug_tuple("SockProto::ProtoOnehundredandtwelve").finish()
        }
        SockProto::ProtoOnehundredandthirteen => {
          f.debug_tuple("SockProto::ProtoOnehundredandthirteen").finish()
        }
        SockProto::ProtoOnehundredandfourteen => {
          f.debug_tuple("SockProto::ProtoOnehundredandfourteen").finish()
        }
        SockProto::ProtoOnehundredandfifteen => {
          f.debug_tuple("SockProto::ProtoOnehundredandfifteen").finish()
        }
        SockProto::ProtoOnehundredandsixteen => {
          f.debug_tuple("SockProto::ProtoOnehundredandsixteen").finish()
        }
        SockProto::ProtoOnehundredandseventeen => {
          f.debug_tuple("SockProto::ProtoOnehundredandseventeen").finish()
        }
        SockProto::ProtoOnehundredandeighteen => {
          f.debug_tuple("SockProto::ProtoOnehundredandeighteen").finish()
        }
        SockProto::ProtoOnehundredandnineteen => {
          f.debug_tuple("SockProto::ProtoOnehundredandnineteen").finish()
        }
        SockProto::ProtoOnehundredandtwenty => {
          f.debug_tuple("SockProto::ProtoOnehundredandtwenty").finish()
        }
        SockProto::ProtoOnehundredandtwentyone => {
          f.debug_tuple("SockProto::ProtoOnehundredandtwentyone").finish()
        }
        SockProto::ProtoOnehundredandtwentytwo => {
          f.debug_tuple("SockProto::ProtoOnehundredandtwentytwo").finish()
        }
        SockProto::ProtoOnehundredandtwentythree => {
          f.debug_tuple("SockProto::ProtoOnehundredandtwentythree").finish()
        }
        SockProto::ProtoOnehundredandtwentyfour => {
          f.debug_tuple("SockProto::ProtoOnehundredandtwentyfour").finish()
        }
        SockProto::ProtoOnehundredandtwentyfive => {
          f.debug_tuple("SockProto::ProtoOnehundredandtwentyfive").finish()
        }
        SockProto::ProtoOnehundredandtwentysix => {
          f.debug_tuple("SockProto::ProtoOnehundredandtwentysix").finish()
        }
        SockProto::ProtoOnehundredandtwentyseven => {
          f.debug_tuple("SockProto::ProtoOnehundredandtwentyseven").finish()
        }
        SockProto::ProtoOnehundredandtwentyeight => {
          f.debug_tuple("SockProto::ProtoOnehundredandtwentyeight").finish()
        }
        SockProto::ProtoOnehundredandtwentynine => {
          f.debug_tuple("SockProto::ProtoOnehundredandtwentynine").finish()
        }
        SockProto::ProtoOnehundredandthirty => {
          f.debug_tuple("SockProto::ProtoOnehundredandthirty").finish()
        }
        SockProto::ProtoOnehundredandthirtyone => {
          f.debug_tuple("SockProto::ProtoOnehundredandthirtyone").finish()
        }
        SockProto::Sctp => {
          f.debug_tuple("SockProto::Sctp").finish()
        }
        SockProto::ProtoOnehundredandthirtythree => {
          f.debug_tuple("SockProto::ProtoOnehundredandthirtythree").finish()
        }
        SockProto::ProtoOnehundredandthirtyfour => {
          f.debug_tuple("SockProto::ProtoOnehundredandthirtyfour").finish()
        }
        SockProto::Mh => {
          f.debug_tuple("SockProto::Mh").finish()
        }
        SockProto::Udplite => {
          f.debug_tuple("SockProto::Udplite").finish()
        }
        SockProto::Mpls => {
          f.debug_tuple("SockProto::Mpls").finish()
        }
        SockProto::ProtoOnehundredandthirtyeight => {
          f.debug_tuple("SockProto::ProtoOnehundredandthirtyeight").finish()
        }
        SockProto::ProtoOnehundredandthirtynine => {
          f.debug_tuple("SockProto::ProtoOnehundredandthirtynine").finish()
        }
        SockProto::ProtoOnehundredandfourty => {
          f.debug_tuple("SockProto::ProtoOnehundredandfourty").finish()
        }
        SockProto::ProtoOnehundredandfourtyone => {
          f.debug_tuple("SockProto::ProtoOnehundredandfourtyone").finish()
        }
        SockProto::ProtoOnehundredandfourtytwo => {
          f.debug_tuple("SockProto::ProtoOnehundredandfourtytwo").finish()
        }
        SockProto::Ethernet => {
          f.debug_tuple("SockProto::Ethernet").finish()
        }
        SockProto::ProtoOnehundredandfourtyfour => {
          f.debug_tuple("SockProto::ProtoOnehundredandfourtyfour").finish()
        }
        SockProto::ProtoOnehundredandfourtyfive => {
          f.debug_tuple("SockProto::ProtoOnehundredandfourtyfive").finish()
        }
        SockProto::ProtoOnehundredandfourtysix => {
          f.debug_tuple("SockProto::ProtoOnehundredandfourtysix").finish()
        }
        SockProto::ProtoOnehundredandfourtyseven => {
          f.debug_tuple("SockProto::ProtoOnehundredandfourtyseven").finish()
        }
        SockProto::ProtoOnehundredandfourtyeight => {
          f.debug_tuple("SockProto::ProtoOnehundredandfourtyeight").finish()
        }
        SockProto::ProtoOnehundredandfourtynine => {
          f.debug_tuple("SockProto::ProtoOnehundredandfourtynine").finish()
        }
        SockProto::ProtoOnehundredandfifty => {
          f.debug_tuple("SockProto::ProtoOnehundredandfifty").finish()
        }
        SockProto::ProtoOnehundredandfiftyone => {
          f.debug_tuple("SockProto::ProtoOnehundredandfiftyone").finish()
        }
        SockProto::ProtoOnehundredandfiftytwo => {
          f.debug_tuple("SockProto::ProtoOnehundredandfiftytwo").finish()
        }
        SockProto::ProtoOnehundredandfiftythree => {
          f.debug_tuple("SockProto::ProtoOnehundredandfiftythree").finish()
        }
        SockProto::ProtoOnehundredandfiftyfour => {
          f.debug_tuple("SockProto::ProtoOnehundredandfiftyfour").finish()
        }
        SockProto::ProtoOnehundredandfiftyfive => {
          f.debug_tuple("SockProto::ProtoOnehundredandfiftyfive").finish()
        }
        SockProto::ProtoOnehundredandfiftysix => {
          f.debug_tuple("SockProto::ProtoOnehundredandfiftysix").finish()
        }
        SockProto::ProtoOnehundredandfiftyseven => {
          f.debug_tuple("SockProto::ProtoOnehundredandfiftyseven").finish()
        }
        SockProto::ProtoOnehundredandfiftyeight => {
          f.debug_tuple("SockProto::ProtoOnehundredandfiftyeight").finish()
        }
        SockProto::ProtoOnehundredandfiftynine => {
          f.debug_tuple("SockProto::ProtoOnehundredandfiftynine").finish()
        }
        SockProto::ProtoOnehundredandsixty => {
          f.debug_tuple("SockProto::ProtoOnehundredandsixty").finish()
        }
        SockProto::ProtoOnehundredandsixtyone => {
          f.debug_tuple("SockProto::ProtoOnehundredandsixtyone").finish()
        }
        SockProto::ProtoOnehundredandsixtytwo => {
          f.debug_tuple("SockProto::ProtoOnehundredandsixtytwo").finish()
        }
        SockProto::ProtoOnehundredandsixtythree => {
          f.debug_tuple("SockProto::ProtoOnehundredandsixtythree").finish()
        }
        SockProto::ProtoOnehundredandsixtyfour => {
          f.debug_tuple("SockProto::ProtoOnehundredandsixtyfour").finish()
        }
        SockProto::ProtoOnehundredandsixtyfive => {
          f.debug_tuple("SockProto::ProtoOnehundredandsixtyfive").finish()
        }
        SockProto::ProtoOnehundredandsixtysix => {
          f.debug_tuple("SockProto::ProtoOnehundredandsixtysix").finish()
        }
        SockProto::ProtoOnehundredandsixtyseven => {
          f.debug_tuple("SockProto::ProtoOnehundredandsixtyseven").finish()
        }
        SockProto::ProtoOnehundredandsixtyeight => {
          f.debug_tuple("SockProto::ProtoOnehundredandsixtyeight").finish()
        }
        SockProto::ProtoOnehundredandsixtynine => {
          f.debug_tuple("SockProto::ProtoOnehundredandsixtynine").finish()
        }
        SockProto::ProtoOnehundredandseventy => {
          f.debug_tuple("SockProto::ProtoOnehundredandseventy").finish()
        }
        SockProto::ProtoOnehundredandseventyone => {
          f.debug_tuple("SockProto::ProtoOnehundredandseventyone").finish()
        }
        SockProto::ProtoOnehundredandseventytwo => {
          f.debug_tuple("SockProto::ProtoOnehundredandseventytwo").finish()
        }
        SockProto::ProtoOnehundredandseventythree => {
          f.debug_tuple("SockProto::ProtoOnehundredandseventythree").finish()
        }
        SockProto::ProtoOnehundredandseventyfour => {
          f.debug_tuple("SockProto::ProtoOnehundredandseventyfour").finish()
        }
        SockProto::ProtoOnehundredandseventyfive => {
          f.debug_tuple("SockProto::ProtoOnehundredandseventyfive").finish()
        }
        SockProto::ProtoOnehundredandseventysix => {
          f.debug_tuple("SockProto::ProtoOnehundredandseventysix").finish()
        }
        SockProto::ProtoOnehundredandseventyseven => {
          f.debug_tuple("SockProto::ProtoOnehundredandseventyseven").finish()
        }
        SockProto::ProtoOnehundredandseventyeight => {
          f.debug_tuple("SockProto::ProtoOnehundredandseventyeight").finish()
        }
        SockProto::ProtoOnehundredandseventynine => {
          f.debug_tuple("SockProto::ProtoOnehundredandseventynine").finish()
        }
        SockProto::ProtoOnehundredandeighty => {
          f.debug_tuple("SockProto::ProtoOnehundredandeighty").finish()
        }
        SockProto::ProtoOnehundredandeightyone => {
          f.debug_tuple("SockProto::ProtoOnehundredandeightyone").finish()
        }
        SockProto::ProtoOnehundredandeightytwo => {
          f.debug_tuple("SockProto::ProtoOnehundredandeightytwo").finish()
        }
        SockProto::ProtoOnehundredandeightythree => {
          f.debug_tuple("SockProto::ProtoOnehundredandeightythree").finish()
        }
        SockProto::ProtoOnehundredandeightyfour => {
          f.debug_tuple("SockProto::ProtoOnehundredandeightyfour").finish()
        }
        SockProto::ProtoOnehundredandeightyfive => {
          f.debug_tuple("SockProto::ProtoOnehundredandeightyfive").finish()
        }
        SockProto::ProtoOnehundredandeightysix => {
          f.debug_tuple("SockProto::ProtoOnehundredandeightysix").finish()
        }
        SockProto::ProtoOnehundredandeightyseven => {
          f.debug_tuple("SockProto::ProtoOnehundredandeightyseven").finish()
        }
        SockProto::ProtoOnehundredandeightyeight => {
          f.debug_tuple("SockProto::ProtoOnehundredandeightyeight").finish()
        }
        SockProto::ProtoOnehundredandeightynine => {
          f.debug_tuple("SockProto::ProtoOnehundredandeightynine").finish()
        }
        SockProto::ProtoOnehundredandninety => {
          f.debug_tuple("SockProto::ProtoOnehundredandninety").finish()
        }
        SockProto::ProtoOnehundredandninetyone => {
          f.debug_tuple("SockProto::ProtoOnehundredandninetyone").finish()
        }
        SockProto::ProtoOnehundredandninetytwo => {
          f.debug_tuple("SockProto::ProtoOnehundredandninetytwo").finish()
        }
        SockProto::ProtoOnehundredandninetythree => {
          f.debug_tuple("SockProto::ProtoOnehundredandninetythree").finish()
        }
        SockProto::ProtoOnehundredandninetyfour => {
          f.debug_tuple("SockProto::ProtoOnehundredandninetyfour").finish()
        }
        SockProto::ProtoOnehundredandninetyfive => {
          f.debug_tuple("SockProto::ProtoOnehundredandninetyfive").finish()
        }
        SockProto::ProtoOnehundredandninetysix => {
          f.debug_tuple("SockProto::ProtoOnehundredandninetysix").finish()
        }
        SockProto::ProtoOnehundredandninetyseven => {
          f.debug_tuple("SockProto::ProtoOnehundredandninetyseven").finish()
        }
        SockProto::ProtoOnehundredandninetyeight => {
          f.debug_tuple("SockProto::ProtoOnehundredandninetyeight").finish()
        }
        SockProto::ProtoOnehundredandninetynine => {
          f.debug_tuple("SockProto::ProtoOnehundredandninetynine").finish()
        }
        SockProto::ProtoTwohundred => {
          f.debug_tuple("SockProto::ProtoTwohundred").finish()
        }
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
        SockProto::ProtoTwohundredandeleven => {
          f.debug_tuple("SockProto::ProtoTwohundredandeleven").finish()
        }
        SockProto::ProtoTwohundredandtwelve => {
          f.debug_tuple("SockProto::ProtoTwohundredandtwelve").finish()
        }
        SockProto::ProtoTwohundredandthirteen => {
          f.debug_tuple("SockProto::ProtoTwohundredandthirteen").finish()
        }
        SockProto::ProtoTwohundredandfourteen => {
          f.debug_tuple("SockProto::ProtoTwohundredandfourteen").finish()
        }
        SockProto::ProtoTwohundredandfifteen => {
          f.debug_tuple("SockProto::ProtoTwohundredandfifteen").finish()
        }
        SockProto::ProtoTwohundredandsixteen => {
          f.debug_tuple("SockProto::ProtoTwohundredandsixteen").finish()
        }
        SockProto::ProtoTwohundredandseventeen => {
          f.debug_tuple("SockProto::ProtoTwohundredandseventeen").finish()
        }
        SockProto::ProtoTwohundredandeighteen => {
          f.debug_tuple("SockProto::ProtoTwohundredandeighteen").finish()
        }
        SockProto::ProtoTwohundredandnineteen => {
          f.debug_tuple("SockProto::ProtoTwohundredandnineteen").finish()
        }
        SockProto::ProtoTwohundredandtwenty => {
          f.debug_tuple("SockProto::ProtoTwohundredandtwenty").finish()
        }
        SockProto::ProtoTwohundredandtwentyone => {
          f.debug_tuple("SockProto::ProtoTwohundredandtwentyone").finish()
        }
        SockProto::ProtoTwohundredandtwentytwo => {
          f.debug_tuple("SockProto::ProtoTwohundredandtwentytwo").finish()
        }
        SockProto::ProtoTwohundredandtwentythree => {
          f.debug_tuple("SockProto::ProtoTwohundredandtwentythree").finish()
        }
        SockProto::ProtoTwohundredandtwentyfour => {
          f.debug_tuple("SockProto::ProtoTwohundredandtwentyfour").finish()
        }
        SockProto::ProtoTwohundredandtwentyfive => {
          f.debug_tuple("SockProto::ProtoTwohundredandtwentyfive").finish()
        }
        SockProto::ProtoTwohundredandtwentysix => {
          f.debug_tuple("SockProto::ProtoTwohundredandtwentysix").finish()
        }
        SockProto::ProtoTwohundredandtwentyseven => {
          f.debug_tuple("SockProto::ProtoTwohundredandtwentyseven").finish()
        }
        SockProto::ProtoTwohundredandtwentyeight => {
          f.debug_tuple("SockProto::ProtoTwohundredandtwentyeight").finish()
        }
        SockProto::ProtoTwohundredandtwentynine => {
          f.debug_tuple("SockProto::ProtoTwohundredandtwentynine").finish()
        }
        SockProto::ProtoTwohundredandthirty => {
          f.debug_tuple("SockProto::ProtoTwohundredandthirty").finish()
        }
        SockProto::ProtoTwohundredandthirtyone => {
          f.debug_tuple("SockProto::ProtoTwohundredandthirtyone").finish()
        }
        SockProto::ProtoTwohundredandthirtytwo => {
          f.debug_tuple("SockProto::ProtoTwohundredandthirtytwo").finish()
        }
        SockProto::ProtoTwohundredandthirtythree => {
          f.debug_tuple("SockProto::ProtoTwohundredandthirtythree").finish()
        }
        SockProto::ProtoTwohundredandthirtyfour => {
          f.debug_tuple("SockProto::ProtoTwohundredandthirtyfour").finish()
        }
        SockProto::ProtoTwohundredandthirtyfive => {
          f.debug_tuple("SockProto::ProtoTwohundredandthirtyfive").finish()
        }
        SockProto::ProtoTwohundredandthirtysix => {
          f.debug_tuple("SockProto::ProtoTwohundredandthirtysix").finish()
        }
        SockProto::ProtoTwohundredandthirtyseven => {
          f.debug_tuple("SockProto::ProtoTwohundredandthirtyseven").finish()
        }
        SockProto::ProtoTwohundredandthirtyeight => {
          f.debug_tuple("SockProto::ProtoTwohundredandthirtyeight").finish()
        }
        SockProto::ProtoTwohundredandthirtynine => {
          f.debug_tuple("SockProto::ProtoTwohundredandthirtynine").finish()
        }
        SockProto::ProtoTwohundredandfourty => {
          f.debug_tuple("SockProto::ProtoTwohundredandfourty").finish()
        }
        SockProto::ProtoTwohundredandfourtyone => {
          f.debug_tuple("SockProto::ProtoTwohundredandfourtyone").finish()
        }
        SockProto::ProtoTwohundredandfourtytwo => {
          f.debug_tuple("SockProto::ProtoTwohundredandfourtytwo").finish()
        }
        SockProto::ProtoTwohundredandfourtythree => {
          f.debug_tuple("SockProto::ProtoTwohundredandfourtythree").finish()
        }
        SockProto::ProtoTwohundredandfourtyfour => {
          f.debug_tuple("SockProto::ProtoTwohundredandfourtyfour").finish()
        }
        SockProto::ProtoTwohundredandfourtyfive => {
          f.debug_tuple("SockProto::ProtoTwohundredandfourtyfive").finish()
        }
        SockProto::ProtoTwohundredandfourtysix => {
          f.debug_tuple("SockProto::ProtoTwohundredandfourtysix").finish()
        }
        SockProto::ProtoTwohundredandfourtyseven => {
          f.debug_tuple("SockProto::ProtoTwohundredandfourtyseven").finish()
        }
        SockProto::ProtoTwohundredandfourtyeight => {
          f.debug_tuple("SockProto::ProtoTwohundredandfourtyeight").finish()
        }
        SockProto::ProtoTwohundredandfourtynine => {
          f.debug_tuple("SockProto::ProtoTwohundredandfourtynine").finish()
        }
        SockProto::ProtoTwohundredandfifty => {
          f.debug_tuple("SockProto::ProtoTwohundredandfifty").finish()
        }
        SockProto::ProtoTwohundredandfiftyone => {
          f.debug_tuple("SockProto::ProtoTwohundredandfiftyone").finish()
        }
        SockProto::ProtoTwohundredandfiftytwo => {
          f.debug_tuple("SockProto::ProtoTwohundredandfiftytwo").finish()
        }
        SockProto::ProtoTwohundredandfiftythree => {
          f.debug_tuple("SockProto::ProtoTwohundredandfiftythree").finish()
        }
        SockProto::ProtoTwohundredandfiftyfour => {
          f.debug_tuple("SockProto::ProtoTwohundredandfiftyfour").finish()
        }
        SockProto::ProtoRaw => {
          f.debug_tuple("SockProto::ProtoRaw").finish()
        }
        SockProto::ProtoTwohundredandfiftysix => {
          f.debug_tuple("SockProto::ProtoTwohundredandfiftysix").finish()
        }
        SockProto::ProtoTwohundredandfiftyseven => {
          f.debug_tuple("SockProto::ProtoTwohundredandfiftyseven").finish()
        }
        SockProto::ProtoTwohundredandfiftyeight => {
          f.debug_tuple("SockProto::ProtoTwohundredandfiftyeight").finish()
        }
        SockProto::ProtoTwohundredandfiftynine => {
          f.debug_tuple("SockProto::ProtoTwohundredandfiftynine").finish()
        }
        SockProto::ProtoTwohundredandsixty => {
          f.debug_tuple("SockProto::ProtoTwohundredandsixty").finish()
        }
        SockProto::ProtoTwohundredandsixtyone => {
          f.debug_tuple("SockProto::ProtoTwohundredandsixtyone").finish()
        }
        SockProto::Mptcp => {
          f.debug_tuple("SockProto::Mptcp").finish()
        }
        SockProto::Max => {
          f.debug_tuple("SockProto::Max").finish()
        }
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
        Bool::False => {
          f.debug_tuple("Bool::False").finish()
        }
        Bool::True => {
          f.debug_tuple("Bool::True").finish()
        }
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
      f.debug_struct("OptionTimestamp").field("tag", &self.tag).field("u", &self.u).finish()}
  }
  #[repr(u8)]
  #[derive(Clone, Copy, PartialEq, Eq)]
  pub enum Signal {
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
        Signal::Sighup => {
          f.debug_tuple("Signal::Sighup").finish()
        }
        Signal::Sigint => {
          f.debug_tuple("Signal::Sigint").finish()
        }
        Signal::Sigquit => {
          f.debug_tuple("Signal::Sigquit").finish()
        }
        Signal::Sigill => {
          f.debug_tuple("Signal::Sigill").finish()
        }
        Signal::Sigtrap => {
          f.debug_tuple("Signal::Sigtrap").finish()
        }
        Signal::Sigabrt => {
          f.debug_tuple("Signal::Sigabrt").finish()
        }
        Signal::Sigbus => {
          f.debug_tuple("Signal::Sigbus").finish()
        }
        Signal::Sigfpe => {
          f.debug_tuple("Signal::Sigfpe").finish()
        }
        Signal::Sigkill => {
          f.debug_tuple("Signal::Sigkill").finish()
        }
        Signal::Sigusr1 => {
          f.debug_tuple("Signal::Sigusr1").finish()
        }
        Signal::Sigsegv => {
          f.debug_tuple("Signal::Sigsegv").finish()
        }
        Signal::Sigusr2 => {
          f.debug_tuple("Signal::Sigusr2").finish()
        }
        Signal::Sigpipe => {
          f.debug_tuple("Signal::Sigpipe").finish()
        }
        Signal::Sigalrm => {
          f.debug_tuple("Signal::Sigalrm").finish()
        }
        Signal::Sigterm => {
          f.debug_tuple("Signal::Sigterm").finish()
        }
        Signal::Sigchld => {
          f.debug_tuple("Signal::Sigchld").finish()
        }
        Signal::Sigcont => {
          f.debug_tuple("Signal::Sigcont").finish()
        }
        Signal::Sigstop => {
          f.debug_tuple("Signal::Sigstop").finish()
        }
        Signal::Sigtstp => {
          f.debug_tuple("Signal::Sigtstp").finish()
        }
        Signal::Sigttin => {
          f.debug_tuple("Signal::Sigttin").finish()
        }
        Signal::Sigttou => {
          f.debug_tuple("Signal::Sigttou").finish()
        }
        Signal::Sigurg => {
          f.debug_tuple("Signal::Sigurg").finish()
        }
        Signal::Sigxcpu => {
          f.debug_tuple("Signal::Sigxcpu").finish()
        }
        Signal::Sigxfsz => {
          f.debug_tuple("Signal::Sigxfsz").finish()
        }
        Signal::Sigvtalrm => {
          f.debug_tuple("Signal::Sigvtalrm").finish()
        }
        Signal::Sigprof => {
          f.debug_tuple("Signal::Sigprof").finish()
        }
        Signal::Sigwinch => {
          f.debug_tuple("Signal::Sigwinch").finish()
        }
        Signal::Sigpoll => {
          f.debug_tuple("Signal::Sigpoll").finish()
        }
        Signal::Sigpwr => {
          f.debug_tuple("Signal::Sigpwr").finish()
        }
        Signal::Sigsys => {
          f.debug_tuple("Signal::Sigsys").finish()
        }
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
      f.debug_struct("AddrUnspec").field("n0", &self.n0).finish()}
  }
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct AddrUnspecPort {
    pub port: u16,
    pub addr: AddrUnspec,
  }
  impl core::fmt::Debug for AddrUnspecPort {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("AddrUnspecPort").field("port", &self.port).field("addr", &self.addr).finish()}
  }
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct CidrUnspec {
    pub addr: AddrUnspec,
    pub prefix: u8,
  }
  impl core::fmt::Debug for CidrUnspec {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("CidrUnspec").field("addr", &self.addr).field("prefix", &self.prefix).finish()}
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
      f.debug_struct("HttpHandles").field("req", &self.req).field("res", &self.res).field("hdr", &self.hdr).finish()}
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
      f.debug_struct("HttpStatus").field("ok", &self.ok).field("redirect", &self.redirect).field("size", &self.size).field("status", &self.status).finish()}
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
  }
  impl core::fmt::Debug for Timeout {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        Timeout::Read => {
          f.debug_tuple("Timeout::Read").finish()
        }
        Timeout::Write => {
          f.debug_tuple("Timeout::Write").finish()
        }
        Timeout::Connect => {
          f.debug_tuple("Timeout::Connect").finish()
        }
        Timeout::Accept => {
          f.debug_tuple("Timeout::Accept").finish()
        }
      }
    }
  }
}
