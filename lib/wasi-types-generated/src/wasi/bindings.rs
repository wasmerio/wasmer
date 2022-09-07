#[allow(clippy::all)]
pub mod output {
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
  /// Dummy function to expose types into generated code
  pub fn expose_types_dummy_func(fd: Fd,dirent: Dirent,snapshot0_dirent: Snapshot0Dirent,snapshot0_event: Snapshot0Event,event_enum: EventEnum,event: Event,fdstat: Fdstat,subscription_clock: SubscriptionClock,snapshot0_subscription_clock: Snapshot0SubscriptionClock,subscription: Subscription,snapshot0_subscription: Snapshot0Subscription,device: Device,linkcount: Linkcount,snapshot0_linkcount: Snapshot0Linkcount,filestat: Filestat,snapshot0_filestat: Snapshot0Filestat,tty: Tty,tid: Tid,pid: Pid,bus_data_format: BusDataFormat,) -> (){
    unsafe {
      let ptr0 = OUTPUT_RET_AREA.0.as_mut_ptr() as i32;
      *((ptr0 + 0) as *mut i32) = wit_bindgen_rust::rt::as_i32(fd);
      let Dirent{ d_next:d_next1, d_ino:d_ino1, d_type:d_type1, d_namlen:d_namlen1, } = dirent;
      *((ptr0 + 8) as *mut i64) = wit_bindgen_rust::rt::as_i64(d_next1);
      *((ptr0 + 16) as *mut i64) = wit_bindgen_rust::rt::as_i64(d_ino1);
      *((ptr0 + 24) as *mut u8) = (match d_type1 {
        Filetype::Unknown => 0,
        Filetype::BlockDevice => 1,
        Filetype::CharacterDevice => 2,
        Filetype::Directory => 3,
        Filetype::RegularFile => 4,
        Filetype::SocketDgram => 5,
        Filetype::SocketStream => 6,
        Filetype::SymbolicLink => 7,
        Filetype::Fifo => 8,
      }) as u8;
      *((ptr0 + 28) as *mut i32) = wit_bindgen_rust::rt::as_i32(d_namlen1);
      let Snapshot0Dirent{ d_next:d_next2, d_ino:d_ino2, d_namlen:d_namlen2, d_type:d_type2, } = snapshot0_dirent;
      *((ptr0 + 32) as *mut i64) = wit_bindgen_rust::rt::as_i64(d_next2);
      *((ptr0 + 40) as *mut i64) = wit_bindgen_rust::rt::as_i64(d_ino2);
      *((ptr0 + 48) as *mut i32) = wit_bindgen_rust::rt::as_i32(d_namlen2);
      *((ptr0 + 52) as *mut u8) = (match d_type2 {
        Filetype::Unknown => 0,
        Filetype::BlockDevice => 1,
        Filetype::CharacterDevice => 2,
        Filetype::Directory => 3,
        Filetype::RegularFile => 4,
        Filetype::SocketDgram => 5,
        Filetype::SocketStream => 6,
        Filetype::SymbolicLink => 7,
        Filetype::Fifo => 8,
      }) as u8;
      let Snapshot0Event{ userdata:userdata3, error:error3, type_:type_3, fd_readwrite:fd_readwrite3, } = snapshot0_event;
      *((ptr0 + 56) as *mut i64) = wit_bindgen_rust::rt::as_i64(userdata3);
      *((ptr0 + 64) as *mut u8) = (match error3 {
        Errno::Success => 0,
        Errno::Toobig => 1,
        Errno::Access => 2,
        Errno::Addrinuse => 3,
        Errno::Addrnotavail => 4,
        Errno::Afnosupport => 5,
        Errno::Again => 6,
        Errno::Already => 7,
        Errno::Badf => 8,
        Errno::Badmsg => 9,
        Errno::Busy => 10,
        Errno::Canceled => 11,
        Errno::Child => 12,
        Errno::Connaborted => 13,
        Errno::Connrefused => 14,
        Errno::Connreset => 15,
        Errno::Deadlk => 16,
        Errno::Destaddrreq => 17,
        Errno::Dom => 18,
        Errno::Dquot => 19,
        Errno::Exist => 20,
        Errno::Fault => 21,
        Errno::Fbig => 22,
        Errno::Hostunreach => 23,
        Errno::Idrm => 24,
        Errno::Ilseq => 25,
        Errno::Inprogress => 26,
        Errno::Intr => 27,
        Errno::Inval => 28,
        Errno::Io => 29,
        Errno::Isconn => 30,
        Errno::Isdir => 31,
        Errno::Loop => 32,
        Errno::Mfile => 33,
        Errno::Mlink => 34,
        Errno::Msgsize => 35,
        Errno::Multihop => 36,
        Errno::Nametoolong => 37,
        Errno::Netdown => 38,
        Errno::Netreset => 39,
        Errno::Netunreach => 40,
        Errno::Nfile => 41,
        Errno::Nobufs => 42,
        Errno::Nodev => 43,
        Errno::Noent => 44,
        Errno::Noexec => 45,
        Errno::Nolck => 46,
        Errno::Nolink => 47,
        Errno::Nomem => 48,
        Errno::Nomsg => 49,
        Errno::Noprotoopt => 50,
        Errno::Nospc => 51,
        Errno::Nosys => 52,
        Errno::Notconn => 53,
        Errno::Notdir => 54,
        Errno::Notempty => 55,
        Errno::Notrecoverable => 56,
        Errno::Notsock => 57,
        Errno::Notsup => 58,
        Errno::Notty => 59,
        Errno::Nxio => 60,
        Errno::Overflow => 61,
        Errno::Ownerdead => 62,
        Errno::Perm => 63,
        Errno::Pipe => 64,
        Errno::Proto => 65,
        Errno::Protonosupport => 66,
        Errno::Prototype => 67,
        Errno::Range => 68,
        Errno::Rofs => 69,
        Errno::Spipe => 70,
        Errno::Srch => 71,
        Errno::Stale => 72,
        Errno::Timedout => 73,
        Errno::Txtbsy => 74,
        Errno::Xdev => 75,
        Errno::Notcapable => 76,
      }) as u8;
      *((ptr0 + 65) as *mut u8) = (match type_3 {
        Eventtype::Clock => 0,
        Eventtype::FdRead => 1,
        Eventtype::FdWrite => 2,
      }) as u8;
      let EventFdReadwrite{ nbytes:nbytes4, flags:flags4, } = fd_readwrite3;
      *((ptr0 + 72) as *mut i64) = wit_bindgen_rust::rt::as_i64(nbytes4);
      let flags5 = flags4;
      *((ptr0 + 80) as *mut u8) = ((flags5.bits() >> 0) as i32) as u8;
      match event_enum {
        EventEnum::FdRead(e) => {
          *((ptr0 + 88) as *mut u8) = (0i32) as u8;
          let EventFdReadwrite{ nbytes:nbytes6, flags:flags6, } = e;
          *((ptr0 + 96) as *mut i64) = wit_bindgen_rust::rt::as_i64(nbytes6);
          let flags7 = flags6;
          *((ptr0 + 104) as *mut u8) = ((flags7.bits() >> 0) as i32) as u8;
          
        },
        EventEnum::FdWrite(e) => {
          *((ptr0 + 88) as *mut u8) = (1i32) as u8;
          let EventFdReadwrite{ nbytes:nbytes8, flags:flags8, } = e;
          *((ptr0 + 96) as *mut i64) = wit_bindgen_rust::rt::as_i64(nbytes8);
          let flags9 = flags8;
          *((ptr0 + 104) as *mut u8) = ((flags9.bits() >> 0) as i32) as u8;
          
        },
        EventEnum::Clock=> {
          let e = ();
          {
            *((ptr0 + 88) as *mut u8) = (2i32) as u8;
            let () = e;
            
          }
        }
      };
      let Event{ userdata:userdata10, error:error10, data:data10, } = event;
      *((ptr0 + 112) as *mut i64) = wit_bindgen_rust::rt::as_i64(userdata10);
      *((ptr0 + 120) as *mut u8) = (match error10 {
        Errno::Success => 0,
        Errno::Toobig => 1,
        Errno::Access => 2,
        Errno::Addrinuse => 3,
        Errno::Addrnotavail => 4,
        Errno::Afnosupport => 5,
        Errno::Again => 6,
        Errno::Already => 7,
        Errno::Badf => 8,
        Errno::Badmsg => 9,
        Errno::Busy => 10,
        Errno::Canceled => 11,
        Errno::Child => 12,
        Errno::Connaborted => 13,
        Errno::Connrefused => 14,
        Errno::Connreset => 15,
        Errno::Deadlk => 16,
        Errno::Destaddrreq => 17,
        Errno::Dom => 18,
        Errno::Dquot => 19,
        Errno::Exist => 20,
        Errno::Fault => 21,
        Errno::Fbig => 22,
        Errno::Hostunreach => 23,
        Errno::Idrm => 24,
        Errno::Ilseq => 25,
        Errno::Inprogress => 26,
        Errno::Intr => 27,
        Errno::Inval => 28,
        Errno::Io => 29,
        Errno::Isconn => 30,
        Errno::Isdir => 31,
        Errno::Loop => 32,
        Errno::Mfile => 33,
        Errno::Mlink => 34,
        Errno::Msgsize => 35,
        Errno::Multihop => 36,
        Errno::Nametoolong => 37,
        Errno::Netdown => 38,
        Errno::Netreset => 39,
        Errno::Netunreach => 40,
        Errno::Nfile => 41,
        Errno::Nobufs => 42,
        Errno::Nodev => 43,
        Errno::Noent => 44,
        Errno::Noexec => 45,
        Errno::Nolck => 46,
        Errno::Nolink => 47,
        Errno::Nomem => 48,
        Errno::Nomsg => 49,
        Errno::Noprotoopt => 50,
        Errno::Nospc => 51,
        Errno::Nosys => 52,
        Errno::Notconn => 53,
        Errno::Notdir => 54,
        Errno::Notempty => 55,
        Errno::Notrecoverable => 56,
        Errno::Notsock => 57,
        Errno::Notsup => 58,
        Errno::Notty => 59,
        Errno::Nxio => 60,
        Errno::Overflow => 61,
        Errno::Ownerdead => 62,
        Errno::Perm => 63,
        Errno::Pipe => 64,
        Errno::Proto => 65,
        Errno::Protonosupport => 66,
        Errno::Prototype => 67,
        Errno::Range => 68,
        Errno::Rofs => 69,
        Errno::Spipe => 70,
        Errno::Srch => 71,
        Errno::Stale => 72,
        Errno::Timedout => 73,
        Errno::Txtbsy => 74,
        Errno::Xdev => 75,
        Errno::Notcapable => 76,
      }) as u8;
      match data10 {
        EventEnum::FdRead(e) => {
          *((ptr0 + 128) as *mut u8) = (0i32) as u8;
          let EventFdReadwrite{ nbytes:nbytes11, flags:flags11, } = e;
          *((ptr0 + 136) as *mut i64) = wit_bindgen_rust::rt::as_i64(nbytes11);
          let flags12 = flags11;
          *((ptr0 + 144) as *mut u8) = ((flags12.bits() >> 0) as i32) as u8;
          
        },
        EventEnum::FdWrite(e) => {
          *((ptr0 + 128) as *mut u8) = (1i32) as u8;
          let EventFdReadwrite{ nbytes:nbytes13, flags:flags13, } = e;
          *((ptr0 + 136) as *mut i64) = wit_bindgen_rust::rt::as_i64(nbytes13);
          let flags14 = flags13;
          *((ptr0 + 144) as *mut u8) = ((flags14.bits() >> 0) as i32) as u8;
          
        },
        EventEnum::Clock=> {
          let e = ();
          {
            *((ptr0 + 128) as *mut u8) = (2i32) as u8;
            let () = e;
            
          }
        }
      };
      let Fdstat{ fs_filetype:fs_filetype15, fs_flags:fs_flags15, fs_rights_base:fs_rights_base15, fs_rights_inheriting:fs_rights_inheriting15, } = fdstat;
      *((ptr0 + 152) as *mut u8) = (match fs_filetype15 {
        Filetype::Unknown => 0,
        Filetype::BlockDevice => 1,
        Filetype::CharacterDevice => 2,
        Filetype::Directory => 3,
        Filetype::RegularFile => 4,
        Filetype::SocketDgram => 5,
        Filetype::SocketStream => 6,
        Filetype::SymbolicLink => 7,
        Filetype::Fifo => 8,
      }) as u8;
      let flags16 = fs_flags15;
      *((ptr0 + 153) as *mut u8) = ((flags16.bits() >> 0) as i32) as u8;
      let flags17 = fs_rights_base15;
      *((ptr0 + 160) as *mut i32) = (flags17.bits() >> 32) as i32;
      *((ptr0 + 156) as *mut i32) = (flags17.bits() >> 0) as i32;
      let flags18 = fs_rights_inheriting15;
      *((ptr0 + 168) as *mut i32) = (flags18.bits() >> 32) as i32;
      *((ptr0 + 164) as *mut i32) = (flags18.bits() >> 0) as i32;
      let SubscriptionClock{ clock_id:clock_id19, timeout:timeout19, precision:precision19, flags:flags19, } = subscription_clock;
      *((ptr0 + 176) as *mut u8) = (match clock_id19 {
        Clockid::Realtime => 0,
        Clockid::Monotonic => 1,
      }) as u8;
      *((ptr0 + 184) as *mut i64) = wit_bindgen_rust::rt::as_i64(timeout19);
      *((ptr0 + 192) as *mut i64) = wit_bindgen_rust::rt::as_i64(precision19);
      let flags20 = flags19;
      *((ptr0 + 200) as *mut u8) = ((flags20.bits() >> 0) as i32) as u8;
      let Snapshot0SubscriptionClock{ identifier:identifier21, id:id21, timeout:timeout21, precision:precision21, flags:flags21, } = snapshot0_subscription_clock;
      *((ptr0 + 208) as *mut i64) = wit_bindgen_rust::rt::as_i64(identifier21);
      *((ptr0 + 216) as *mut u8) = (match id21 {
        Snapshot0Clockid::Realtime => 0,
        Snapshot0Clockid::Monotonic => 1,
        Snapshot0Clockid::ProcessCputimeId => 2,
        Snapshot0Clockid::ThreadCputimeId => 3,
      }) as u8;
      *((ptr0 + 224) as *mut i64) = wit_bindgen_rust::rt::as_i64(timeout21);
      *((ptr0 + 232) as *mut i64) = wit_bindgen_rust::rt::as_i64(precision21);
      let flags22 = flags21;
      *((ptr0 + 240) as *mut u8) = ((flags22.bits() >> 0) as i32) as u8;
      let Subscription{ userdata:userdata23, data:data23, } = subscription;
      *((ptr0 + 248) as *mut i64) = wit_bindgen_rust::rt::as_i64(userdata23);
      match data23 {
        SubscriptionEnum::Clock(e) => {
          *((ptr0 + 256) as *mut u8) = (0i32) as u8;
          let SubscriptionClock{ clock_id:clock_id24, timeout:timeout24, precision:precision24, flags:flags24, } = e;
          *((ptr0 + 264) as *mut u8) = (match clock_id24 {
            Clockid::Realtime => 0,
            Clockid::Monotonic => 1,
          }) as u8;
          *((ptr0 + 272) as *mut i64) = wit_bindgen_rust::rt::as_i64(timeout24);
          *((ptr0 + 280) as *mut i64) = wit_bindgen_rust::rt::as_i64(precision24);
          let flags25 = flags24;
          *((ptr0 + 288) as *mut u8) = ((flags25.bits() >> 0) as i32) as u8;
          
        },
        SubscriptionEnum::Read(e) => {
          *((ptr0 + 256) as *mut u8) = (1i32) as u8;
          let SubscriptionFsReadwrite{ file_descriptor:file_descriptor26, } = e;
          *((ptr0 + 264) as *mut i32) = wit_bindgen_rust::rt::as_i32(file_descriptor26);
          
        },
        SubscriptionEnum::Write(e) => {
          *((ptr0 + 256) as *mut u8) = (2i32) as u8;
          let SubscriptionFsReadwrite{ file_descriptor:file_descriptor27, } = e;
          *((ptr0 + 264) as *mut i32) = wit_bindgen_rust::rt::as_i32(file_descriptor27);
          
        },
      };
      let Snapshot0Subscription{ userdata:userdata28, data:data28, } = snapshot0_subscription;
      *((ptr0 + 296) as *mut i64) = wit_bindgen_rust::rt::as_i64(userdata28);
      match data28 {
        Snapshot0SubscriptionEnum::Clock(e) => {
          *((ptr0 + 304) as *mut u8) = (0i32) as u8;
          let Snapshot0SubscriptionClock{ identifier:identifier29, id:id29, timeout:timeout29, precision:precision29, flags:flags29, } = e;
          *((ptr0 + 312) as *mut i64) = wit_bindgen_rust::rt::as_i64(identifier29);
          *((ptr0 + 320) as *mut u8) = (match id29 {
            Snapshot0Clockid::Realtime => 0,
            Snapshot0Clockid::Monotonic => 1,
            Snapshot0Clockid::ProcessCputimeId => 2,
            Snapshot0Clockid::ThreadCputimeId => 3,
          }) as u8;
          *((ptr0 + 328) as *mut i64) = wit_bindgen_rust::rt::as_i64(timeout29);
          *((ptr0 + 336) as *mut i64) = wit_bindgen_rust::rt::as_i64(precision29);
          let flags30 = flags29;
          *((ptr0 + 344) as *mut u8) = ((flags30.bits() >> 0) as i32) as u8;
          
        },
        Snapshot0SubscriptionEnum::Read(e) => {
          *((ptr0 + 304) as *mut u8) = (1i32) as u8;
          let SubscriptionFsReadwrite{ file_descriptor:file_descriptor31, } = e;
          *((ptr0 + 312) as *mut i32) = wit_bindgen_rust::rt::as_i32(file_descriptor31);
          
        },
        Snapshot0SubscriptionEnum::Write(e) => {
          *((ptr0 + 304) as *mut u8) = (2i32) as u8;
          let SubscriptionFsReadwrite{ file_descriptor:file_descriptor32, } = e;
          *((ptr0 + 312) as *mut i32) = wit_bindgen_rust::rt::as_i32(file_descriptor32);
          
        },
      };
      *((ptr0 + 352) as *mut i64) = wit_bindgen_rust::rt::as_i64(device);
      *((ptr0 + 360) as *mut i64) = wit_bindgen_rust::rt::as_i64(linkcount);
      *((ptr0 + 368) as *mut i32) = wit_bindgen_rust::rt::as_i32(snapshot0_linkcount);
      let Filestat{ st_dev:st_dev33, st_ino:st_ino33, st_filetype:st_filetype33, st_nlink:st_nlink33, st_size:st_size33, st_atim:st_atim33, st_mtim:st_mtim33, st_ctim:st_ctim33, } = filestat;
      *((ptr0 + 376) as *mut i64) = wit_bindgen_rust::rt::as_i64(st_dev33);
      *((ptr0 + 384) as *mut i64) = wit_bindgen_rust::rt::as_i64(st_ino33);
      *((ptr0 + 392) as *mut u8) = (match st_filetype33 {
        Filetype::Unknown => 0,
        Filetype::BlockDevice => 1,
        Filetype::CharacterDevice => 2,
        Filetype::Directory => 3,
        Filetype::RegularFile => 4,
        Filetype::SocketDgram => 5,
        Filetype::SocketStream => 6,
        Filetype::SymbolicLink => 7,
        Filetype::Fifo => 8,
      }) as u8;
      *((ptr0 + 400) as *mut i64) = wit_bindgen_rust::rt::as_i64(st_nlink33);
      *((ptr0 + 408) as *mut i64) = wit_bindgen_rust::rt::as_i64(st_size33);
      *((ptr0 + 416) as *mut i64) = wit_bindgen_rust::rt::as_i64(st_atim33);
      *((ptr0 + 424) as *mut i64) = wit_bindgen_rust::rt::as_i64(st_mtim33);
      *((ptr0 + 432) as *mut i64) = wit_bindgen_rust::rt::as_i64(st_ctim33);
      let Snapshot0Filestat{ st_dev:st_dev34, st_ino:st_ino34, st_filetype:st_filetype34, st_nlink:st_nlink34, st_size:st_size34, st_atim:st_atim34, st_mtim:st_mtim34, st_ctim:st_ctim34, } = snapshot0_filestat;
      *((ptr0 + 440) as *mut i64) = wit_bindgen_rust::rt::as_i64(st_dev34);
      *((ptr0 + 448) as *mut i64) = wit_bindgen_rust::rt::as_i64(st_ino34);
      *((ptr0 + 456) as *mut u8) = (match st_filetype34 {
        Filetype::Unknown => 0,
        Filetype::BlockDevice => 1,
        Filetype::CharacterDevice => 2,
        Filetype::Directory => 3,
        Filetype::RegularFile => 4,
        Filetype::SocketDgram => 5,
        Filetype::SocketStream => 6,
        Filetype::SymbolicLink => 7,
        Filetype::Fifo => 8,
      }) as u8;
      *((ptr0 + 460) as *mut i32) = wit_bindgen_rust::rt::as_i32(st_nlink34);
      *((ptr0 + 464) as *mut i64) = wit_bindgen_rust::rt::as_i64(st_size34);
      *((ptr0 + 472) as *mut i64) = wit_bindgen_rust::rt::as_i64(st_atim34);
      *((ptr0 + 480) as *mut i64) = wit_bindgen_rust::rt::as_i64(st_mtim34);
      *((ptr0 + 488) as *mut i64) = wit_bindgen_rust::rt::as_i64(st_ctim34);
      let Tty{ cols:cols35, rows:rows35, width:width35, height:height35, stdin_tty:stdin_tty35, stdout_tty:stdout_tty35, stderr_tty:stderr_tty35, echo:echo35, line_buffered:line_buffered35, } = tty;
      *((ptr0 + 496) as *mut i32) = wit_bindgen_rust::rt::as_i32(cols35);
      *((ptr0 + 500) as *mut i32) = wit_bindgen_rust::rt::as_i32(rows35);
      *((ptr0 + 504) as *mut i32) = wit_bindgen_rust::rt::as_i32(width35);
      *((ptr0 + 508) as *mut i32) = wit_bindgen_rust::rt::as_i32(height35);
      *((ptr0 + 512) as *mut u8) = (match stdin_tty35 { true => 1, false => 0 }) as u8;
      *((ptr0 + 513) as *mut u8) = (match stdout_tty35 { true => 1, false => 0 }) as u8;
      *((ptr0 + 514) as *mut u8) = (match stderr_tty35 { true => 1, false => 0 }) as u8;
      *((ptr0 + 515) as *mut u8) = (match echo35 { true => 1, false => 0 }) as u8;
      *((ptr0 + 516) as *mut u8) = (match line_buffered35 { true => 1, false => 0 }) as u8;
      *((ptr0 + 520) as *mut i32) = wit_bindgen_rust::rt::as_i32(tid);
      *((ptr0 + 524) as *mut i32) = wit_bindgen_rust::rt::as_i32(pid);
      *((ptr0 + 528) as *mut u8) = (match bus_data_format {
        BusDataFormat::Raw => 0,
        BusDataFormat::Bincode => 1,
        BusDataFormat::MessagePack => 2,
        BusDataFormat::Json => 3,
        BusDataFormat::Yaml => 4,
        BusDataFormat::Xml => 5,
        BusDataFormat::Rkyv => 6,
      }) as u8;
      #[link(wasm_import_module = "output")]
      extern "C" {
        #[cfg_attr(target_arch = "wasm32", link_name = "expose-types-dummy-func")]
        #[cfg_attr(not(target_arch = "wasm32"), link_name = "output_expose-types-dummy-func")]
        fn wit_import(_: i32, );
      }
      wit_import(ptr0);
      ()
    }
  }
  
  #[repr(align(8))]
  struct RetArea([u8; 536]);
  static mut OUTPUT_RET_AREA: RetArea = RetArea([0; 536]);
}
