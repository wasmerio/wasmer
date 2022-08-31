#[allow(clippy::all)]
pub mod wasi_filesystem {
  #[allow(unused_imports)]
  use wit_bindgen_wasmer::{anyhow, wasmer};
  /// Non-negative file size or length of a region within a file.
  pub type Filesize = u64;
  /// Relative offset within a file.
  pub type Filedelta = i64;
  /// Timestamp in nanoseconds.
  /// 
  /// TODO: wasi-clocks is moving to seconds+nanoseconds.
  pub type Timestamp = u64;
  /// Information associated with a descriptor.
  /// 
  /// Note: This was called `fdstat` in earlier versions of WASI.
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct Info {
    /// The type of filesystem object referenced by a descriptor.
    pub type_: Type,
    /// Flags associated with a descriptor.
    pub flags: Flags,
  }
  impl core::fmt::Debug for Info {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("Info").field("type", &self.type_).field("flags", &self.flags).finish()}
  }
  /// The type of a filesystem object referenced by a descriptor.
  /// 
  /// Note: This was called `filetype` in earlier versions of WASI.
  #[repr(u8)]
  #[derive(Clone, Copy, PartialEq, Eq)]
  pub enum Type {
    /// The type of the descriptor or file is unknown or is different from
    /// any of the other types specified.
    Unknown,
    /// The descriptor refers to a block device inode.
    BlockDevice,
    /// The descriptor refers to a character device inode.
    CharacterDevice,
    /// The descriptor refers to a directory inode.
    Directory,
    /// The descriptor refers to a named pipe.
    Fifo,
    /// The file refers to a symbolic link inode.
    SymbolicLink,
    /// The descriptor refers to a regular file inode.
    RegularFile,
    /// The descriptor refers to a socket.
    Socket,
  }
  impl core::fmt::Debug for Type {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        Type::Unknown => {
          f.debug_tuple("Type::Unknown").finish()
        }
        Type::BlockDevice => {
          f.debug_tuple("Type::BlockDevice").finish()
        }
        Type::CharacterDevice => {
          f.debug_tuple("Type::CharacterDevice").finish()
        }
        Type::Directory => {
          f.debug_tuple("Type::Directory").finish()
        }
        Type::Fifo => {
          f.debug_tuple("Type::Fifo").finish()
        }
        Type::SymbolicLink => {
          f.debug_tuple("Type::SymbolicLink").finish()
        }
        Type::RegularFile => {
          f.debug_tuple("Type::RegularFile").finish()
        }
        Type::Socket => {
          f.debug_tuple("Type::Socket").finish()
        }
      }
    }
  }
  wit_bindgen_wasmer::bitflags::bitflags! {
    /// Descriptor flags.
    /// 
    /// Note: This was called `fd-flags` in earlier versions of WASI.
    pub struct Flags: u8 {/// Read mode: Data can be read.
      const READ = 1 << 0;
      /// Write mode: Data can be written to.
      const WRITE = 1 << 1;
      /// Append mode: Data written to the file is always appended to the file's
      /// end.
      const APPEND = 1 << 2;
      /// Write according to synchronized I/O data integrity completion. Only the
      /// data stored in the file is synchronized.
      const DSYNC = 1 << 3;
      /// Non-blocking mode.
      const NONBLOCK = 1 << 4;
      /// Synchronized read I/O operations.
      const RSYNC = 1 << 5;
      /// Write according to synchronized I/O file integrity completion. In
      /// addition to synchronizing the data stored in the file, the
      /// implementation may also synchronously update the file's metadata.
      const SYNC = 1 << 6;
    }
  }
  
  impl core::fmt::Display for Flags{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.write_str("Flags(")?;
      core::fmt::Debug::fmt(self, f)?;
      f.write_str(" (0x")?;
      core::fmt::LowerHex::fmt(&self.bits, f)?;
      f.write_str("))")?;
      Ok(())}
  }
  
  /// File attributes.
  /// 
  /// Note: This was called `filestat` in earlier versions of WASI.
  #[repr(C)]
  #[derive(Copy, Clone)]
  pub struct Stat {
    /// Device ID of device containing the file.
    pub dev: Device,
    /// File serial number.
    pub ino: Inode,
    /// File type.
    pub type_: Type,
    /// Number of hard links to the file.
    pub nlink: Linkcount,
    /// For regular files, the file size in bytes. For symbolic links, the length
    /// in bytes of the pathname contained in the symbolic link.
    pub size: Filesize,
    /// Last data access timestamp.
    pub atim: Timestamp,
    /// Last data modification timestamp.
    pub mtim: Timestamp,
    /// Last file status change timestamp.
    pub ctim: Timestamp,
  }
  impl core::fmt::Debug for Stat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.debug_struct("Stat").field("dev", &self.dev).field("ino", &self.ino).field("type", &self.type_).field("nlink", &self.nlink).field("size", &self.size).field("atim", &self.atim).field("mtim", &self.mtim).field("ctim", &self.ctim).finish()}
  }
  wit_bindgen_wasmer::bitflags::bitflags! {
    /// Flags determining the method of how paths are resolved.
    pub struct AtFlags: u8 {/// As long as the resolved path corresponds to a symbolic link, it is expanded.
      const SYMLINK_FOLLOW = 1 << 0;
    }
  }
  
  impl core::fmt::Display for AtFlags{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.write_str("AtFlags(")?;
      core::fmt::Debug::fmt(self, f)?;
      f.write_str(" (0x")?;
      core::fmt::LowerHex::fmt(&self.bits, f)?;
      f.write_str("))")?;
      Ok(())}
  }
  
  wit_bindgen_wasmer::bitflags::bitflags! {
    /// Open flags used by `open-at`.
    pub struct OFlags: u8 {/// Create file if it does not exist.
      const CREATE = 1 << 0;
      /// Fail if not a directory.
      const DIRECTORY = 1 << 1;
      /// Fail if file already exists.
      const EXCL = 1 << 2;
      /// Truncate file to size 0.
      const TRUNC = 1 << 3;
    }
  }
  
  impl core::fmt::Display for OFlags{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.write_str("OFlags(")?;
      core::fmt::Debug::fmt(self, f)?;
      f.write_str(" (0x")?;
      core::fmt::LowerHex::fmt(&self.bits, f)?;
      f.write_str("))")?;
      Ok(())}
  }
  
  wit_bindgen_wasmer::bitflags::bitflags! {
    /// Permissions mode used by `open-at`, `change-permissions-at`, and similar.
    pub struct Mode: u8 {/// True if the resource is considered readable by the containing
      /// filesystem.
      const READABLE = 1 << 0;
      /// True if the resource is considered writeable by the containing
      /// filesystem.
      const WRITEABLE = 1 << 1;
      /// True if the resource is considered executable by the containing
      /// filesystem. This does not apply to directories.
      const EXECUTABLE = 1 << 2;
    }
  }
  
  impl core::fmt::Display for Mode{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.write_str("Mode(")?;
      core::fmt::Debug::fmt(self, f)?;
      f.write_str(" (0x")?;
      core::fmt::LowerHex::fmt(&self.bits, f)?;
      f.write_str("))")?;
      Ok(())}
  }
  
  /// Number of hard links to an inode.
  pub type Linkcount = u64;
  /// Identifier for a device containing a file system. Can be used in combination
  /// with `inode` to uniquely identify a file or directory in the filesystem.
  pub type Device = u64;
  /// Filesystem object serial number that is unique within its file system.
  pub type Inode = u64;
  /// When setting a timestamp, this gives the value to set it to.
  #[derive(Clone, Copy)]
  pub enum NewTimestamp{
    /// Leave the timestamp set to its previous value.
    NoChange,
    /// Set the timestamp to the current time of the system clock associated
    /// with the filesystem.
    Now,
    /// Set the timestamp to the given value.
    Timestamp(Timestamp),
  }
  impl core::fmt::Debug for NewTimestamp {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        NewTimestamp::NoChange => {
          f.debug_tuple("NewTimestamp::NoChange").finish()
        }
        NewTimestamp::Now => {
          f.debug_tuple("NewTimestamp::Now").finish()
        }
        NewTimestamp::Timestamp(e) => {
          f.debug_tuple("NewTimestamp::Timestamp").field(e).finish()
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
    /// Argument list too long. This is similar to `E2BIG` in POSIX.
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
      }
    }
    pub fn message(&self) -> &'static str {
      match self {
        Errno::Success => "No error occurred. System call completed successfully.",
        Errno::Toobig => "Argument list too long. This is similar to `E2BIG` in POSIX.",
        Errno::Access => "Permission denied.",
        Errno::Addrinuse => "Address in use.",
        Errno::Addrnotavail => "Address not available.",
        Errno::Afnosupport => "Address family not supported.",
        Errno::Again => "Resource unavailable, or operation would block.",
        Errno::Already => "Connection already in progress.",
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
    WillNeed,
    /// The application expects that it will not access the specified data in the near future.
    DontNeed,
    /// The application expects to access the specified data once and then not reuse it thereafter.
    NoReuse,
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
        Advice::WillNeed => {
          f.debug_tuple("Advice::WillNeed").finish()
        }
        Advice::DontNeed => {
          f.debug_tuple("Advice::DontNeed").finish()
        }
        Advice::NoReuse => {
          f.debug_tuple("Advice::NoReuse").finish()
        }
      }
    }
  }
  /// The position relative to which to set the offset of the descriptor.
  #[derive(Clone, Copy)]
  pub enum SeekFrom{
    /// Seek relative to start-of-file.
    Set(Filesize),
    /// Seek relative to current position.
    Cur(Filedelta),
    /// Seek relative to end-of-file.
    End(Filesize),
  }
  impl core::fmt::Debug for SeekFrom {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        SeekFrom::Set(e) => {
          f.debug_tuple("SeekFrom::Set").field(e).finish()
        }
        SeekFrom::Cur(e) => {
          f.debug_tuple("SeekFrom::Cur").field(e).finish()
        }
        SeekFrom::End(e) => {
          f.debug_tuple("SeekFrom::End").field(e).finish()
        }
      }
    }
  }
  /// A descriptor is a reference to a filesystem object, which may be a file,
  /// directory, named pipe, special file, or other object on which filesystem
  /// calls may be made.
  #[derive(Debug)]
  pub struct Descriptor(wit_bindgen_wasmer::rt::ResourceIndex);
  
  /// Auxiliary data associated with the wasm exports.
  #[derive(Default)]
  pub struct WasiFilesystemData {
    
    index_slab0: wit_bindgen_wasmer::rt::IndexSlab,
    resource_slab0: wit_bindgen_wasmer::rt::ResourceSlab,
    dtor0: OnceCell<wasmer::TypedFunction<i32, ()>>,
  }
  
  pub struct WasiFilesystem {
    #[allow(dead_code)]
    env: wasmer::FunctionEnv<WasiFilesystemData>,
    func_canonical_abi_free: wasmer::TypedFunction<(i32, i32, i32), ()>,
    func_canonical_abi_realloc: wasmer::TypedFunction<(i32, i32, i32, i32), i32>,
    func_descriptor_change_directory_permissions_at: wasmer::TypedFunction<(i32,i32,i32,i32,i32,), i32>,
    func_descriptor_change_file_permissions_at: wasmer::TypedFunction<(i32,i32,i32,i32,i32,), i32>,
    func_descriptor_create_directory_at: wasmer::TypedFunction<(i32,i32,i32,), i32>,
    func_descriptor_datasync: wasmer::TypedFunction<i32, i32>,
    func_descriptor_fadvise: wasmer::TypedFunction<(i32,i64,i64,i32,), i32>,
    func_descriptor_fallocate: wasmer::TypedFunction<(i32,i64,i64,), i32>,
    func_descriptor_info: wasmer::TypedFunction<i32, i32>,
    func_descriptor_link_at: wasmer::TypedFunction<(i32,i32,i32,i32,i32,i32,i32,), i32>,
    func_descriptor_open_at: wasmer::TypedFunction<(i32,i32,i32,i32,i32,i32,i32,), i32>,
    func_descriptor_readlink_at: wasmer::TypedFunction<(i32,i32,i32,), i32>,
    func_descriptor_remove_directory_at: wasmer::TypedFunction<(i32,i32,i32,), i32>,
    func_descriptor_rename_at: wasmer::TypedFunction<(i32,i32,i32,i32,i32,i32,), i32>,
    func_descriptor_seek: wasmer::TypedFunction<(i32,i32,i64,), i32>,
    func_descriptor_set_size: wasmer::TypedFunction<(i32,i64,), i32>,
    func_descriptor_set_times: wasmer::TypedFunction<(i32,i32,i64,i32,i64,), i32>,
    func_descriptor_set_times_at: wasmer::TypedFunction<(i32,i32,i32,i32,i32,i64,i32,i64,), i32>,
    func_descriptor_stat_at: wasmer::TypedFunction<(i32,i32,i32,i32,), i32>,
    func_descriptor_symlink_at: wasmer::TypedFunction<(i32,i32,i32,i32,i32,), i32>,
    func_descriptor_sync: wasmer::TypedFunction<i32, i32>,
    func_descriptor_tell: wasmer::TypedFunction<i32, i32>,
    func_descriptor_unlink_file_at: wasmer::TypedFunction<(i32,i32,i32,), i32>,
    memory: wasmer::Memory,
  }
  impl WasiFilesystem {
    
    /// Adds any intrinsics, if necessary for this exported wasm
    /// functionality to the `ImportObject` provided.
    ///
    /// This function returns the `WasiFilesystemData` which needs to be
    /// passed through to `WasiFilesystem::new`.
    fn add_to_imports(
    mut store: impl wasmer::AsStoreMut,
    imports: &mut wasmer::Imports,
    ) -> wasmer::FunctionEnv<WasiFilesystemData> {
      let env = wasmer::FunctionEnv::new(&mut store, WasiFilesystemData::default());
      let mut canonical_abi = imports.get_namespace_exports("canonical_abi").unwrap_or_else(wasmer::Exports::new);
      
      canonical_abi.insert(
      "resource_drop_descriptor",
      wasmer::Function::new_typed_with_env(
      &mut store,
      &env,
      move |mut store: wasmer::FunctionEnvMut<WasiFilesystemData>, idx: u32| -> Result<(), wasmer::RuntimeError> {
        let resource_idx = store.data_mut().index_slab0.remove(idx)?;
        let wasm = match store.data_mut().resource_slab0.drop(resource_idx) {
          Some(wasm) => wasm,
          None => return Ok(()),
        };
        let dtor = store.data_mut().dtor0.get().unwrap().clone();
        dtor.call(&mut store, wasm)?;
        Ok(())
      },
      )
      );
      canonical_abi.insert(
      "resource_clone_descriptor",
      wasmer::Function::new_typed_with_env(
      &mut store,
      &env,
      move |mut store: wasmer::FunctionEnvMut<WasiFilesystemData>, idx: u32| -> Result<u32, wasmer::RuntimeError>  {
        let state = &mut *store.data_mut();
        let resource_idx = state.index_slab0.get(idx)?;
        state.resource_slab0.clone(resource_idx)?;
        Ok(state.index_slab0.insert(resource_idx))
      },
      )
      );
      canonical_abi.insert(
      "resource_get_descriptor",
      wasmer::Function::new_typed_with_env(
      &mut store,
      &env,
      move |mut store: wasmer::FunctionEnvMut<WasiFilesystemData>, idx: u32| -> Result<i32, wasmer::RuntimeError>  {
        let state = &mut *store.data_mut();
        let resource_idx = state.index_slab0.get(idx)?;
        Ok(state.resource_slab0.get(resource_idx))
      },
      )
      );
      canonical_abi.insert(
      "resource_new_descriptor",
      wasmer::Function::new_typed_with_env(
      &mut store,
      &env,
      move |mut store: wasmer::FunctionEnvMut<WasiFilesystemData>, val: i32| -> Result<u32, wasmer::RuntimeError>  {
        let state = &mut *store.data_mut();
        let resource_idx = state.resource_slab0.insert(val);
        Ok(state.index_slab0.insert(resource_idx))
      },
      )
      );
      imports.register_namespace("canonical_abi", canonical_abi);
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
      {
        let dtor0 = instance
        .exports
        .get_typed_function(
        &store,
        "canonical_abi_drop_descriptor",
        )?
        .clone();
        
        env
        .as_mut(&mut store)
        .dtor0
        .set(dtor0)
        .map_err(|_e| anyhow::anyhow!("Couldn't set canonical_abi_drop_descriptor"))?;
      }
      
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
    env: wasmer::FunctionEnv<WasiFilesystemData>,
    ) -> Result<Self, wasmer::ExportError> {
      let func_canonical_abi_free= _instance.exports.get_typed_function(&store, "canonical_abi_free")?;
      let func_canonical_abi_realloc= _instance.exports.get_typed_function(&store, "canonical_abi_realloc")?;
      let func_descriptor_change_directory_permissions_at= _instance.exports.get_typed_function(&store, "descriptor::change-directory-permissions-at")?;
      let func_descriptor_change_file_permissions_at= _instance.exports.get_typed_function(&store, "descriptor::change-file-permissions-at")?;
      let func_descriptor_create_directory_at= _instance.exports.get_typed_function(&store, "descriptor::create-directory-at")?;
      let func_descriptor_datasync= _instance.exports.get_typed_function(&store, "descriptor::datasync")?;
      let func_descriptor_fadvise= _instance.exports.get_typed_function(&store, "descriptor::fadvise")?;
      let func_descriptor_fallocate= _instance.exports.get_typed_function(&store, "descriptor::fallocate")?;
      let func_descriptor_info= _instance.exports.get_typed_function(&store, "descriptor::info")?;
      let func_descriptor_link_at= _instance.exports.get_typed_function(&store, "descriptor::link-at")?;
      let func_descriptor_open_at= _instance.exports.get_typed_function(&store, "descriptor::open-at")?;
      let func_descriptor_readlink_at= _instance.exports.get_typed_function(&store, "descriptor::readlink-at")?;
      let func_descriptor_remove_directory_at= _instance.exports.get_typed_function(&store, "descriptor::remove-directory-at")?;
      let func_descriptor_rename_at= _instance.exports.get_typed_function(&store, "descriptor::rename-at")?;
      let func_descriptor_seek= _instance.exports.get_typed_function(&store, "descriptor::seek")?;
      let func_descriptor_set_size= _instance.exports.get_typed_function(&store, "descriptor::set-size")?;
      let func_descriptor_set_times= _instance.exports.get_typed_function(&store, "descriptor::set-times")?;
      let func_descriptor_set_times_at= _instance.exports.get_typed_function(&store, "descriptor::set-times-at")?;
      let func_descriptor_stat_at= _instance.exports.get_typed_function(&store, "descriptor::stat-at")?;
      let func_descriptor_symlink_at= _instance.exports.get_typed_function(&store, "descriptor::symlink-at")?;
      let func_descriptor_sync= _instance.exports.get_typed_function(&store, "descriptor::sync")?;
      let func_descriptor_tell= _instance.exports.get_typed_function(&store, "descriptor::tell")?;
      let func_descriptor_unlink_file_at= _instance.exports.get_typed_function(&store, "descriptor::unlink-file-at")?;
      let memory= _instance.exports.get_memory("memory")?.clone();
      Ok(WasiFilesystem{
        func_canonical_abi_free,
        func_canonical_abi_realloc,
        func_descriptor_change_directory_permissions_at,
        func_descriptor_change_file_permissions_at,
        func_descriptor_create_directory_at,
        func_descriptor_datasync,
        func_descriptor_fadvise,
        func_descriptor_fallocate,
        func_descriptor_info,
        func_descriptor_link_at,
        func_descriptor_open_at,
        func_descriptor_readlink_at,
        func_descriptor_remove_directory_at,
        func_descriptor_rename_at,
        func_descriptor_seek,
        func_descriptor_set_size,
        func_descriptor_set_times,
        func_descriptor_set_times_at,
        func_descriptor_stat_at,
        func_descriptor_symlink_at,
        func_descriptor_sync,
        func_descriptor_tell,
        func_descriptor_unlink_file_at,
        memory,
        env,
      })
    }
    /// Provide file advisory information on a descriptor.
    /// 
    /// This is similar to `posix_fadvise` in POSIX.
    pub fn descriptor_fadvise(&self, store: &mut wasmer::Store,self_: & Descriptor,offset: u64,len: u64,advice: Advice,)-> Result<Result<(),Errno>, wasmer::RuntimeError> {
      let _memory = &self.memory;
      
      let obj0 = self_;
      let handle0 = {
        let state = self.env.as_mut(store);
        state.resource_slab0.clone(obj0.0)?;
        state.index_slab0.insert(obj0.0)
      };
      let result1 = self.func_descriptor_fadvise.call(store, handle0 as i32, wit_bindgen_wasmer::rt::as_i64(offset), wit_bindgen_wasmer::rt::as_i64(len), advice as i32, )?;
      let _memory_view = _memory.view(&store);
      let load2 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result1 + 0)?;
      Ok(match i32::from(load2) {
        0 => Ok(()),
        1 => Err({
          let _memory_view = _memory.view(&store);
          let load3 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result1 + 1)?;
          match i32::from(load3) {
            0 => Errno::Success,
            1 => Errno::Toobig,
            2 => Errno::Access,
            3 => Errno::Addrinuse,
            4 => Errno::Addrnotavail,
            5 => Errno::Afnosupport,
            6 => Errno::Again,
            7 => Errno::Already,
            8 => Errno::Badmsg,
            9 => Errno::Busy,
            10 => Errno::Canceled,
            11 => Errno::Child,
            12 => Errno::Connaborted,
            13 => Errno::Connrefused,
            14 => Errno::Connreset,
            15 => Errno::Deadlk,
            16 => Errno::Destaddrreq,
            17 => Errno::Dom,
            18 => Errno::Dquot,
            19 => Errno::Exist,
            20 => Errno::Fault,
            21 => Errno::Fbig,
            22 => Errno::Hostunreach,
            23 => Errno::Idrm,
            24 => Errno::Ilseq,
            25 => Errno::Inprogress,
            26 => Errno::Intr,
            27 => Errno::Inval,
            28 => Errno::Io,
            29 => Errno::Isconn,
            30 => Errno::Isdir,
            31 => Errno::Loop,
            32 => Errno::Mfile,
            33 => Errno::Mlink,
            34 => Errno::Msgsize,
            35 => Errno::Multihop,
            36 => Errno::Nametoolong,
            37 => Errno::Netdown,
            38 => Errno::Netreset,
            39 => Errno::Netunreach,
            40 => Errno::Nfile,
            41 => Errno::Nobufs,
            42 => Errno::Nodev,
            43 => Errno::Noent,
            44 => Errno::Noexec,
            45 => Errno::Nolck,
            46 => Errno::Nolink,
            47 => Errno::Nomem,
            48 => Errno::Nomsg,
            49 => Errno::Noprotoopt,
            50 => Errno::Nospc,
            51 => Errno::Nosys,
            52 => Errno::Notconn,
            53 => Errno::Notdir,
            54 => Errno::Notempty,
            55 => Errno::Notrecoverable,
            56 => Errno::Notsock,
            57 => Errno::Notsup,
            58 => Errno::Notty,
            59 => Errno::Nxio,
            60 => Errno::Overflow,
            61 => Errno::Ownerdead,
            62 => Errno::Perm,
            63 => Errno::Pipe,
            64 => Errno::Proto,
            65 => Errno::Protonosupport,
            66 => Errno::Prototype,
            67 => Errno::Range,
            68 => Errno::Rofs,
            69 => Errno::Spipe,
            70 => Errno::Srch,
            71 => Errno::Stale,
            72 => Errno::Timedout,
            73 => Errno::Txtbsy,
            74 => Errno::Xdev,
            _ => return Err(invalid_variant("Errno")),
          }
        }),
        _ => return Err(invalid_variant("expected")),
      })
    }
    /// Force the allocation of space in a file.
    /// 
    /// Note: This is similar to `posix_fallocate` in POSIX.
    pub fn descriptor_fallocate(&self, store: &mut wasmer::Store,self_: & Descriptor,offset: Filesize,len: Filesize,)-> Result<Result<(),Errno>, wasmer::RuntimeError> {
      let _memory = &self.memory;
      
      let obj0 = self_;
      let handle0 = {
        let state = self.env.as_mut(store);
        state.resource_slab0.clone(obj0.0)?;
        state.index_slab0.insert(obj0.0)
      };
      let result1 = self.func_descriptor_fallocate.call(store, handle0 as i32, wit_bindgen_wasmer::rt::as_i64(offset), wit_bindgen_wasmer::rt::as_i64(len), )?;
      let _memory_view = _memory.view(&store);
      let load2 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result1 + 0)?;
      Ok(match i32::from(load2) {
        0 => Ok(()),
        1 => Err({
          let _memory_view = _memory.view(&store);
          let load3 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result1 + 1)?;
          match i32::from(load3) {
            0 => Errno::Success,
            1 => Errno::Toobig,
            2 => Errno::Access,
            3 => Errno::Addrinuse,
            4 => Errno::Addrnotavail,
            5 => Errno::Afnosupport,
            6 => Errno::Again,
            7 => Errno::Already,
            8 => Errno::Badmsg,
            9 => Errno::Busy,
            10 => Errno::Canceled,
            11 => Errno::Child,
            12 => Errno::Connaborted,
            13 => Errno::Connrefused,
            14 => Errno::Connreset,
            15 => Errno::Deadlk,
            16 => Errno::Destaddrreq,
            17 => Errno::Dom,
            18 => Errno::Dquot,
            19 => Errno::Exist,
            20 => Errno::Fault,
            21 => Errno::Fbig,
            22 => Errno::Hostunreach,
            23 => Errno::Idrm,
            24 => Errno::Ilseq,
            25 => Errno::Inprogress,
            26 => Errno::Intr,
            27 => Errno::Inval,
            28 => Errno::Io,
            29 => Errno::Isconn,
            30 => Errno::Isdir,
            31 => Errno::Loop,
            32 => Errno::Mfile,
            33 => Errno::Mlink,
            34 => Errno::Msgsize,
            35 => Errno::Multihop,
            36 => Errno::Nametoolong,
            37 => Errno::Netdown,
            38 => Errno::Netreset,
            39 => Errno::Netunreach,
            40 => Errno::Nfile,
            41 => Errno::Nobufs,
            42 => Errno::Nodev,
            43 => Errno::Noent,
            44 => Errno::Noexec,
            45 => Errno::Nolck,
            46 => Errno::Nolink,
            47 => Errno::Nomem,
            48 => Errno::Nomsg,
            49 => Errno::Noprotoopt,
            50 => Errno::Nospc,
            51 => Errno::Nosys,
            52 => Errno::Notconn,
            53 => Errno::Notdir,
            54 => Errno::Notempty,
            55 => Errno::Notrecoverable,
            56 => Errno::Notsock,
            57 => Errno::Notsup,
            58 => Errno::Notty,
            59 => Errno::Nxio,
            60 => Errno::Overflow,
            61 => Errno::Ownerdead,
            62 => Errno::Perm,
            63 => Errno::Pipe,
            64 => Errno::Proto,
            65 => Errno::Protonosupport,
            66 => Errno::Prototype,
            67 => Errno::Range,
            68 => Errno::Rofs,
            69 => Errno::Spipe,
            70 => Errno::Srch,
            71 => Errno::Stale,
            72 => Errno::Timedout,
            73 => Errno::Txtbsy,
            74 => Errno::Xdev,
            _ => return Err(invalid_variant("Errno")),
          }
        }),
        _ => return Err(invalid_variant("expected")),
      })
    }
    /// Synchronize the data of a file to disk.
    /// 
    /// Note: This is similar to `fdatasync` in POSIX.
    pub fn descriptor_datasync(&self, store: &mut wasmer::Store,self_: & Descriptor,)-> Result<Result<(),Errno>, wasmer::RuntimeError> {
      let _memory = &self.memory;
      
      let obj0 = self_;
      let handle0 = {
        let state = self.env.as_mut(store);
        state.resource_slab0.clone(obj0.0)?;
        state.index_slab0.insert(obj0.0)
      };
      let result1 = self.func_descriptor_datasync.call(store, handle0 as i32, )?;
      let _memory_view = _memory.view(&store);
      let load2 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result1 + 0)?;
      Ok(match i32::from(load2) {
        0 => Ok(()),
        1 => Err({
          let _memory_view = _memory.view(&store);
          let load3 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result1 + 1)?;
          match i32::from(load3) {
            0 => Errno::Success,
            1 => Errno::Toobig,
            2 => Errno::Access,
            3 => Errno::Addrinuse,
            4 => Errno::Addrnotavail,
            5 => Errno::Afnosupport,
            6 => Errno::Again,
            7 => Errno::Already,
            8 => Errno::Badmsg,
            9 => Errno::Busy,
            10 => Errno::Canceled,
            11 => Errno::Child,
            12 => Errno::Connaborted,
            13 => Errno::Connrefused,
            14 => Errno::Connreset,
            15 => Errno::Deadlk,
            16 => Errno::Destaddrreq,
            17 => Errno::Dom,
            18 => Errno::Dquot,
            19 => Errno::Exist,
            20 => Errno::Fault,
            21 => Errno::Fbig,
            22 => Errno::Hostunreach,
            23 => Errno::Idrm,
            24 => Errno::Ilseq,
            25 => Errno::Inprogress,
            26 => Errno::Intr,
            27 => Errno::Inval,
            28 => Errno::Io,
            29 => Errno::Isconn,
            30 => Errno::Isdir,
            31 => Errno::Loop,
            32 => Errno::Mfile,
            33 => Errno::Mlink,
            34 => Errno::Msgsize,
            35 => Errno::Multihop,
            36 => Errno::Nametoolong,
            37 => Errno::Netdown,
            38 => Errno::Netreset,
            39 => Errno::Netunreach,
            40 => Errno::Nfile,
            41 => Errno::Nobufs,
            42 => Errno::Nodev,
            43 => Errno::Noent,
            44 => Errno::Noexec,
            45 => Errno::Nolck,
            46 => Errno::Nolink,
            47 => Errno::Nomem,
            48 => Errno::Nomsg,
            49 => Errno::Noprotoopt,
            50 => Errno::Nospc,
            51 => Errno::Nosys,
            52 => Errno::Notconn,
            53 => Errno::Notdir,
            54 => Errno::Notempty,
            55 => Errno::Notrecoverable,
            56 => Errno::Notsock,
            57 => Errno::Notsup,
            58 => Errno::Notty,
            59 => Errno::Nxio,
            60 => Errno::Overflow,
            61 => Errno::Ownerdead,
            62 => Errno::Perm,
            63 => Errno::Pipe,
            64 => Errno::Proto,
            65 => Errno::Protonosupport,
            66 => Errno::Prototype,
            67 => Errno::Range,
            68 => Errno::Rofs,
            69 => Errno::Spipe,
            70 => Errno::Srch,
            71 => Errno::Stale,
            72 => Errno::Timedout,
            73 => Errno::Txtbsy,
            74 => Errno::Xdev,
            _ => return Err(invalid_variant("Errno")),
          }
        }),
        _ => return Err(invalid_variant("expected")),
      })
    }
    /// Get information associated with a descriptor.
    /// 
    /// Note: This returns similar flags to `fsync(fd, F_GETFL)` in POSIX, as well
    /// as additional fields.
    /// 
    /// Note: This was called `fdstat_get` in earlier versions of WASI.
    pub fn descriptor_info(&self, store: &mut wasmer::Store,self_: & Descriptor,)-> Result<Result<Info,Errno>, wasmer::RuntimeError> {
      let _memory = &self.memory;
      
      let obj0 = self_;
      let handle0 = {
        let state = self.env.as_mut(store);
        state.resource_slab0.clone(obj0.0)?;
        state.index_slab0.insert(obj0.0)
      };
      let result1 = self.func_descriptor_info.call(store, handle0 as i32, )?;
      let _memory_view = _memory.view(&store);
      let load2 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result1 + 0)?;
      Ok(match i32::from(load2) {
        0 => Ok({
          let _memory_view = _memory.view(&store);
          let load3 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result1 + 1)?;
          let _memory_view = _memory.view(&store);
          let load4 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result1 + 2)?;
          Info{type_:match i32::from(load3) {
            0 => Type::Unknown,
            1 => Type::BlockDevice,
            2 => Type::CharacterDevice,
            3 => Type::Directory,
            4 => Type::Fifo,
            5 => Type::SymbolicLink,
            6 => Type::RegularFile,
            7 => Type::Socket,
            _ => return Err(invalid_variant("Type")),
          }, flags:validate_flags(
          0| ((i32::from(load4) as u8) << 0),
          Flags::all().bits(),
          "Flags",
          |bits| Flags { bits }
          )?, }
        }),
        1 => Err({
          let _memory_view = _memory.view(&store);
          let load5 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result1 + 1)?;
          match i32::from(load5) {
            0 => Errno::Success,
            1 => Errno::Toobig,
            2 => Errno::Access,
            3 => Errno::Addrinuse,
            4 => Errno::Addrnotavail,
            5 => Errno::Afnosupport,
            6 => Errno::Again,
            7 => Errno::Already,
            8 => Errno::Badmsg,
            9 => Errno::Busy,
            10 => Errno::Canceled,
            11 => Errno::Child,
            12 => Errno::Connaborted,
            13 => Errno::Connrefused,
            14 => Errno::Connreset,
            15 => Errno::Deadlk,
            16 => Errno::Destaddrreq,
            17 => Errno::Dom,
            18 => Errno::Dquot,
            19 => Errno::Exist,
            20 => Errno::Fault,
            21 => Errno::Fbig,
            22 => Errno::Hostunreach,
            23 => Errno::Idrm,
            24 => Errno::Ilseq,
            25 => Errno::Inprogress,
            26 => Errno::Intr,
            27 => Errno::Inval,
            28 => Errno::Io,
            29 => Errno::Isconn,
            30 => Errno::Isdir,
            31 => Errno::Loop,
            32 => Errno::Mfile,
            33 => Errno::Mlink,
            34 => Errno::Msgsize,
            35 => Errno::Multihop,
            36 => Errno::Nametoolong,
            37 => Errno::Netdown,
            38 => Errno::Netreset,
            39 => Errno::Netunreach,
            40 => Errno::Nfile,
            41 => Errno::Nobufs,
            42 => Errno::Nodev,
            43 => Errno::Noent,
            44 => Errno::Noexec,
            45 => Errno::Nolck,
            46 => Errno::Nolink,
            47 => Errno::Nomem,
            48 => Errno::Nomsg,
            49 => Errno::Noprotoopt,
            50 => Errno::Nospc,
            51 => Errno::Nosys,
            52 => Errno::Notconn,
            53 => Errno::Notdir,
            54 => Errno::Notempty,
            55 => Errno::Notrecoverable,
            56 => Errno::Notsock,
            57 => Errno::Notsup,
            58 => Errno::Notty,
            59 => Errno::Nxio,
            60 => Errno::Overflow,
            61 => Errno::Ownerdead,
            62 => Errno::Perm,
            63 => Errno::Pipe,
            64 => Errno::Proto,
            65 => Errno::Protonosupport,
            66 => Errno::Prototype,
            67 => Errno::Range,
            68 => Errno::Rofs,
            69 => Errno::Spipe,
            70 => Errno::Srch,
            71 => Errno::Stale,
            72 => Errno::Timedout,
            73 => Errno::Txtbsy,
            74 => Errno::Xdev,
            _ => return Err(invalid_variant("Errno")),
          }
        }),
        _ => return Err(invalid_variant("expected")),
      })
    }
    /// Adjust the size of an open file. If this increases the file's size, the
    /// extra bytes are filled with zeros.
    /// 
    /// Note: This was called `fd_filestat_set_size` in earlier versions of WASI.
    pub fn descriptor_set_size(&self, store: &mut wasmer::Store,self_: & Descriptor,size: Filesize,)-> Result<Result<(),Errno>, wasmer::RuntimeError> {
      let _memory = &self.memory;
      
      let obj0 = self_;
      let handle0 = {
        let state = self.env.as_mut(store);
        state.resource_slab0.clone(obj0.0)?;
        state.index_slab0.insert(obj0.0)
      };
      let result1 = self.func_descriptor_set_size.call(store, handle0 as i32, wit_bindgen_wasmer::rt::as_i64(size), )?;
      let _memory_view = _memory.view(&store);
      let load2 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result1 + 0)?;
      Ok(match i32::from(load2) {
        0 => Ok(()),
        1 => Err({
          let _memory_view = _memory.view(&store);
          let load3 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result1 + 1)?;
          match i32::from(load3) {
            0 => Errno::Success,
            1 => Errno::Toobig,
            2 => Errno::Access,
            3 => Errno::Addrinuse,
            4 => Errno::Addrnotavail,
            5 => Errno::Afnosupport,
            6 => Errno::Again,
            7 => Errno::Already,
            8 => Errno::Badmsg,
            9 => Errno::Busy,
            10 => Errno::Canceled,
            11 => Errno::Child,
            12 => Errno::Connaborted,
            13 => Errno::Connrefused,
            14 => Errno::Connreset,
            15 => Errno::Deadlk,
            16 => Errno::Destaddrreq,
            17 => Errno::Dom,
            18 => Errno::Dquot,
            19 => Errno::Exist,
            20 => Errno::Fault,
            21 => Errno::Fbig,
            22 => Errno::Hostunreach,
            23 => Errno::Idrm,
            24 => Errno::Ilseq,
            25 => Errno::Inprogress,
            26 => Errno::Intr,
            27 => Errno::Inval,
            28 => Errno::Io,
            29 => Errno::Isconn,
            30 => Errno::Isdir,
            31 => Errno::Loop,
            32 => Errno::Mfile,
            33 => Errno::Mlink,
            34 => Errno::Msgsize,
            35 => Errno::Multihop,
            36 => Errno::Nametoolong,
            37 => Errno::Netdown,
            38 => Errno::Netreset,
            39 => Errno::Netunreach,
            40 => Errno::Nfile,
            41 => Errno::Nobufs,
            42 => Errno::Nodev,
            43 => Errno::Noent,
            44 => Errno::Noexec,
            45 => Errno::Nolck,
            46 => Errno::Nolink,
            47 => Errno::Nomem,
            48 => Errno::Nomsg,
            49 => Errno::Noprotoopt,
            50 => Errno::Nospc,
            51 => Errno::Nosys,
            52 => Errno::Notconn,
            53 => Errno::Notdir,
            54 => Errno::Notempty,
            55 => Errno::Notrecoverable,
            56 => Errno::Notsock,
            57 => Errno::Notsup,
            58 => Errno::Notty,
            59 => Errno::Nxio,
            60 => Errno::Overflow,
            61 => Errno::Ownerdead,
            62 => Errno::Perm,
            63 => Errno::Pipe,
            64 => Errno::Proto,
            65 => Errno::Protonosupport,
            66 => Errno::Prototype,
            67 => Errno::Range,
            68 => Errno::Rofs,
            69 => Errno::Spipe,
            70 => Errno::Srch,
            71 => Errno::Stale,
            72 => Errno::Timedout,
            73 => Errno::Txtbsy,
            74 => Errno::Xdev,
            _ => return Err(invalid_variant("Errno")),
          }
        }),
        _ => return Err(invalid_variant("expected")),
      })
    }
    /// Adjust the timestamps of an open file or directory.
    /// 
    /// Note: This is similar to `futimens` in POSIX.
    /// 
    /// Note: This was called `fd_filestat_set_times` in earlier versions of WASI.
    pub fn descriptor_set_times(&self, store: &mut wasmer::Store,self_: & Descriptor,atim: NewTimestamp,mtim: NewTimestamp,)-> Result<Result<(),Errno>, wasmer::RuntimeError> {
      let _memory = &self.memory;
      
      let obj0 = self_;
      let handle0 = {
        let state = self.env.as_mut(store);
        state.resource_slab0.clone(obj0.0)?;
        state.index_slab0.insert(obj0.0)
      };
      let (result1_0,result1_1,) = match atim {
        NewTimestamp::NoChange=> {
          let e = ();
          {
            let () = e;
            (0i32, 0i64)
          }
        }
        NewTimestamp::Now=> {
          let e = ();
          {
            let () = e;
            (1i32, 0i64)
          }
        }
        NewTimestamp::Timestamp(e) => (2i32, wit_bindgen_wasmer::rt::as_i64(e)),
      };
      let (result2_0,result2_1,) = match mtim {
        NewTimestamp::NoChange=> {
          let e = ();
          {
            let () = e;
            (0i32, 0i64)
          }
        }
        NewTimestamp::Now=> {
          let e = ();
          {
            let () = e;
            (1i32, 0i64)
          }
        }
        NewTimestamp::Timestamp(e) => (2i32, wit_bindgen_wasmer::rt::as_i64(e)),
      };
      let result3 = self.func_descriptor_set_times.call(store, handle0 as i32, result1_0, result1_1, result2_0, result2_1, )?;
      let _memory_view = _memory.view(&store);
      let load4 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result3 + 0)?;
      Ok(match i32::from(load4) {
        0 => Ok(()),
        1 => Err({
          let _memory_view = _memory.view(&store);
          let load5 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result3 + 1)?;
          match i32::from(load5) {
            0 => Errno::Success,
            1 => Errno::Toobig,
            2 => Errno::Access,
            3 => Errno::Addrinuse,
            4 => Errno::Addrnotavail,
            5 => Errno::Afnosupport,
            6 => Errno::Again,
            7 => Errno::Already,
            8 => Errno::Badmsg,
            9 => Errno::Busy,
            10 => Errno::Canceled,
            11 => Errno::Child,
            12 => Errno::Connaborted,
            13 => Errno::Connrefused,
            14 => Errno::Connreset,
            15 => Errno::Deadlk,
            16 => Errno::Destaddrreq,
            17 => Errno::Dom,
            18 => Errno::Dquot,
            19 => Errno::Exist,
            20 => Errno::Fault,
            21 => Errno::Fbig,
            22 => Errno::Hostunreach,
            23 => Errno::Idrm,
            24 => Errno::Ilseq,
            25 => Errno::Inprogress,
            26 => Errno::Intr,
            27 => Errno::Inval,
            28 => Errno::Io,
            29 => Errno::Isconn,
            30 => Errno::Isdir,
            31 => Errno::Loop,
            32 => Errno::Mfile,
            33 => Errno::Mlink,
            34 => Errno::Msgsize,
            35 => Errno::Multihop,
            36 => Errno::Nametoolong,
            37 => Errno::Netdown,
            38 => Errno::Netreset,
            39 => Errno::Netunreach,
            40 => Errno::Nfile,
            41 => Errno::Nobufs,
            42 => Errno::Nodev,
            43 => Errno::Noent,
            44 => Errno::Noexec,
            45 => Errno::Nolck,
            46 => Errno::Nolink,
            47 => Errno::Nomem,
            48 => Errno::Nomsg,
            49 => Errno::Noprotoopt,
            50 => Errno::Nospc,
            51 => Errno::Nosys,
            52 => Errno::Notconn,
            53 => Errno::Notdir,
            54 => Errno::Notempty,
            55 => Errno::Notrecoverable,
            56 => Errno::Notsock,
            57 => Errno::Notsup,
            58 => Errno::Notty,
            59 => Errno::Nxio,
            60 => Errno::Overflow,
            61 => Errno::Ownerdead,
            62 => Errno::Perm,
            63 => Errno::Pipe,
            64 => Errno::Proto,
            65 => Errno::Protonosupport,
            66 => Errno::Prototype,
            67 => Errno::Range,
            68 => Errno::Rofs,
            69 => Errno::Spipe,
            70 => Errno::Srch,
            71 => Errno::Stale,
            72 => Errno::Timedout,
            73 => Errno::Txtbsy,
            74 => Errno::Xdev,
            _ => return Err(invalid_variant("Errno")),
          }
        }),
        _ => return Err(invalid_variant("expected")),
      })
    }
    /// Move the offset of a descriptor.
    /// 
    /// The meaning of `seek` on a directory is unspecified.
    /// 
    /// Returns new offset of the descriptor, relative to the start of the file.
    /// 
    /// Note: This is similar to `lseek` in POSIX.
    pub fn descriptor_seek(&self, store: &mut wasmer::Store,self_: & Descriptor,from: SeekFrom,)-> Result<Result<Filesize,Errno>, wasmer::RuntimeError> {
      let _memory = &self.memory;
      
      let obj0 = self_;
      let handle0 = {
        let state = self.env.as_mut(store);
        state.resource_slab0.clone(obj0.0)?;
        state.index_slab0.insert(obj0.0)
      };
      let (result1_0,result1_1,) = match from {
        SeekFrom::Set(e) => (0i32, wit_bindgen_wasmer::rt::as_i64(e)),
        SeekFrom::Cur(e) => (1i32, wit_bindgen_wasmer::rt::as_i64(e)),
        SeekFrom::End(e) => (2i32, wit_bindgen_wasmer::rt::as_i64(e)),
      };
      let result2 = self.func_descriptor_seek.call(store, handle0 as i32, result1_0, result1_1, )?;
      let _memory_view = _memory.view(&store);
      let load3 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result2 + 0)?;
      Ok(match i32::from(load3) {
        0 => Ok({
          let _memory_view = _memory.view(&store);
          let load4 = unsafe { _memory_view.data_unchecked_mut() }.load::<i64>(result2 + 8)?;
          load4 as u64
        }),
        1 => Err({
          let _memory_view = _memory.view(&store);
          let load5 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result2 + 8)?;
          match i32::from(load5) {
            0 => Errno::Success,
            1 => Errno::Toobig,
            2 => Errno::Access,
            3 => Errno::Addrinuse,
            4 => Errno::Addrnotavail,
            5 => Errno::Afnosupport,
            6 => Errno::Again,
            7 => Errno::Already,
            8 => Errno::Badmsg,
            9 => Errno::Busy,
            10 => Errno::Canceled,
            11 => Errno::Child,
            12 => Errno::Connaborted,
            13 => Errno::Connrefused,
            14 => Errno::Connreset,
            15 => Errno::Deadlk,
            16 => Errno::Destaddrreq,
            17 => Errno::Dom,
            18 => Errno::Dquot,
            19 => Errno::Exist,
            20 => Errno::Fault,
            21 => Errno::Fbig,
            22 => Errno::Hostunreach,
            23 => Errno::Idrm,
            24 => Errno::Ilseq,
            25 => Errno::Inprogress,
            26 => Errno::Intr,
            27 => Errno::Inval,
            28 => Errno::Io,
            29 => Errno::Isconn,
            30 => Errno::Isdir,
            31 => Errno::Loop,
            32 => Errno::Mfile,
            33 => Errno::Mlink,
            34 => Errno::Msgsize,
            35 => Errno::Multihop,
            36 => Errno::Nametoolong,
            37 => Errno::Netdown,
            38 => Errno::Netreset,
            39 => Errno::Netunreach,
            40 => Errno::Nfile,
            41 => Errno::Nobufs,
            42 => Errno::Nodev,
            43 => Errno::Noent,
            44 => Errno::Noexec,
            45 => Errno::Nolck,
            46 => Errno::Nolink,
            47 => Errno::Nomem,
            48 => Errno::Nomsg,
            49 => Errno::Noprotoopt,
            50 => Errno::Nospc,
            51 => Errno::Nosys,
            52 => Errno::Notconn,
            53 => Errno::Notdir,
            54 => Errno::Notempty,
            55 => Errno::Notrecoverable,
            56 => Errno::Notsock,
            57 => Errno::Notsup,
            58 => Errno::Notty,
            59 => Errno::Nxio,
            60 => Errno::Overflow,
            61 => Errno::Ownerdead,
            62 => Errno::Perm,
            63 => Errno::Pipe,
            64 => Errno::Proto,
            65 => Errno::Protonosupport,
            66 => Errno::Prototype,
            67 => Errno::Range,
            68 => Errno::Rofs,
            69 => Errno::Spipe,
            70 => Errno::Srch,
            71 => Errno::Stale,
            72 => Errno::Timedout,
            73 => Errno::Txtbsy,
            74 => Errno::Xdev,
            _ => return Err(invalid_variant("Errno")),
          }
        }),
        _ => return Err(invalid_variant("expected")),
      })
    }
    /// Synchronize the data and metadata of a file to disk.
    /// 
    /// Note: This is similar to `fsync` in POSIX.
    pub fn descriptor_sync(&self, store: &mut wasmer::Store,self_: & Descriptor,)-> Result<Result<(),Errno>, wasmer::RuntimeError> {
      let _memory = &self.memory;
      
      let obj0 = self_;
      let handle0 = {
        let state = self.env.as_mut(store);
        state.resource_slab0.clone(obj0.0)?;
        state.index_slab0.insert(obj0.0)
      };
      let result1 = self.func_descriptor_sync.call(store, handle0 as i32, )?;
      let _memory_view = _memory.view(&store);
      let load2 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result1 + 0)?;
      Ok(match i32::from(load2) {
        0 => Ok(()),
        1 => Err({
          let _memory_view = _memory.view(&store);
          let load3 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result1 + 1)?;
          match i32::from(load3) {
            0 => Errno::Success,
            1 => Errno::Toobig,
            2 => Errno::Access,
            3 => Errno::Addrinuse,
            4 => Errno::Addrnotavail,
            5 => Errno::Afnosupport,
            6 => Errno::Again,
            7 => Errno::Already,
            8 => Errno::Badmsg,
            9 => Errno::Busy,
            10 => Errno::Canceled,
            11 => Errno::Child,
            12 => Errno::Connaborted,
            13 => Errno::Connrefused,
            14 => Errno::Connreset,
            15 => Errno::Deadlk,
            16 => Errno::Destaddrreq,
            17 => Errno::Dom,
            18 => Errno::Dquot,
            19 => Errno::Exist,
            20 => Errno::Fault,
            21 => Errno::Fbig,
            22 => Errno::Hostunreach,
            23 => Errno::Idrm,
            24 => Errno::Ilseq,
            25 => Errno::Inprogress,
            26 => Errno::Intr,
            27 => Errno::Inval,
            28 => Errno::Io,
            29 => Errno::Isconn,
            30 => Errno::Isdir,
            31 => Errno::Loop,
            32 => Errno::Mfile,
            33 => Errno::Mlink,
            34 => Errno::Msgsize,
            35 => Errno::Multihop,
            36 => Errno::Nametoolong,
            37 => Errno::Netdown,
            38 => Errno::Netreset,
            39 => Errno::Netunreach,
            40 => Errno::Nfile,
            41 => Errno::Nobufs,
            42 => Errno::Nodev,
            43 => Errno::Noent,
            44 => Errno::Noexec,
            45 => Errno::Nolck,
            46 => Errno::Nolink,
            47 => Errno::Nomem,
            48 => Errno::Nomsg,
            49 => Errno::Noprotoopt,
            50 => Errno::Nospc,
            51 => Errno::Nosys,
            52 => Errno::Notconn,
            53 => Errno::Notdir,
            54 => Errno::Notempty,
            55 => Errno::Notrecoverable,
            56 => Errno::Notsock,
            57 => Errno::Notsup,
            58 => Errno::Notty,
            59 => Errno::Nxio,
            60 => Errno::Overflow,
            61 => Errno::Ownerdead,
            62 => Errno::Perm,
            63 => Errno::Pipe,
            64 => Errno::Proto,
            65 => Errno::Protonosupport,
            66 => Errno::Prototype,
            67 => Errno::Range,
            68 => Errno::Rofs,
            69 => Errno::Spipe,
            70 => Errno::Srch,
            71 => Errno::Stale,
            72 => Errno::Timedout,
            73 => Errno::Txtbsy,
            74 => Errno::Xdev,
            _ => return Err(invalid_variant("Errno")),
          }
        }),
        _ => return Err(invalid_variant("expected")),
      })
    }
    /// Return the current offset of a descriptor.
    /// 
    /// Returns the current offset of the descriptor, relative to the start of the file.
    /// 
    /// Note: This is similar to `lseek(fd, 0, SEEK_CUR)` in POSIX.
    pub fn descriptor_tell(&self, store: &mut wasmer::Store,self_: & Descriptor,)-> Result<Result<Filesize,Errno>, wasmer::RuntimeError> {
      let _memory = &self.memory;
      
      let obj0 = self_;
      let handle0 = {
        let state = self.env.as_mut(store);
        state.resource_slab0.clone(obj0.0)?;
        state.index_slab0.insert(obj0.0)
      };
      let result1 = self.func_descriptor_tell.call(store, handle0 as i32, )?;
      let _memory_view = _memory.view(&store);
      let load2 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result1 + 0)?;
      Ok(match i32::from(load2) {
        0 => Ok({
          let _memory_view = _memory.view(&store);
          let load3 = unsafe { _memory_view.data_unchecked_mut() }.load::<i64>(result1 + 8)?;
          load3 as u64
        }),
        1 => Err({
          let _memory_view = _memory.view(&store);
          let load4 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result1 + 8)?;
          match i32::from(load4) {
            0 => Errno::Success,
            1 => Errno::Toobig,
            2 => Errno::Access,
            3 => Errno::Addrinuse,
            4 => Errno::Addrnotavail,
            5 => Errno::Afnosupport,
            6 => Errno::Again,
            7 => Errno::Already,
            8 => Errno::Badmsg,
            9 => Errno::Busy,
            10 => Errno::Canceled,
            11 => Errno::Child,
            12 => Errno::Connaborted,
            13 => Errno::Connrefused,
            14 => Errno::Connreset,
            15 => Errno::Deadlk,
            16 => Errno::Destaddrreq,
            17 => Errno::Dom,
            18 => Errno::Dquot,
            19 => Errno::Exist,
            20 => Errno::Fault,
            21 => Errno::Fbig,
            22 => Errno::Hostunreach,
            23 => Errno::Idrm,
            24 => Errno::Ilseq,
            25 => Errno::Inprogress,
            26 => Errno::Intr,
            27 => Errno::Inval,
            28 => Errno::Io,
            29 => Errno::Isconn,
            30 => Errno::Isdir,
            31 => Errno::Loop,
            32 => Errno::Mfile,
            33 => Errno::Mlink,
            34 => Errno::Msgsize,
            35 => Errno::Multihop,
            36 => Errno::Nametoolong,
            37 => Errno::Netdown,
            38 => Errno::Netreset,
            39 => Errno::Netunreach,
            40 => Errno::Nfile,
            41 => Errno::Nobufs,
            42 => Errno::Nodev,
            43 => Errno::Noent,
            44 => Errno::Noexec,
            45 => Errno::Nolck,
            46 => Errno::Nolink,
            47 => Errno::Nomem,
            48 => Errno::Nomsg,
            49 => Errno::Noprotoopt,
            50 => Errno::Nospc,
            51 => Errno::Nosys,
            52 => Errno::Notconn,
            53 => Errno::Notdir,
            54 => Errno::Notempty,
            55 => Errno::Notrecoverable,
            56 => Errno::Notsock,
            57 => Errno::Notsup,
            58 => Errno::Notty,
            59 => Errno::Nxio,
            60 => Errno::Overflow,
            61 => Errno::Ownerdead,
            62 => Errno::Perm,
            63 => Errno::Pipe,
            64 => Errno::Proto,
            65 => Errno::Protonosupport,
            66 => Errno::Prototype,
            67 => Errno::Range,
            68 => Errno::Rofs,
            69 => Errno::Spipe,
            70 => Errno::Srch,
            71 => Errno::Stale,
            72 => Errno::Timedout,
            73 => Errno::Txtbsy,
            74 => Errno::Xdev,
            _ => return Err(invalid_variant("Errno")),
          }
        }),
        _ => return Err(invalid_variant("expected")),
      })
    }
    /// Create a directory.
    /// 
    /// Note: This is similar to `mkdirat` in POSIX.
    pub fn descriptor_create_directory_at(&self, store: &mut wasmer::Store,self_: & Descriptor,path: & str,)-> Result<Result<(),Errno>, wasmer::RuntimeError> {
      let func_canonical_abi_realloc = &self.func_canonical_abi_realloc;
      let _memory = &self.memory;
      
      let obj0 = self_;
      let handle0 = {
        let state = self.env.as_mut(store);
        state.resource_slab0.clone(obj0.0)?;
        state.index_slab0.insert(obj0.0)
      };
      let vec1 = path;
      let ptr1 = func_canonical_abi_realloc.call(&mut store.as_store_mut(), 0, 0, 1, vec1.len() as i32)?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store_many(ptr1, vec1.as_bytes())?;
      let result2 = self.func_descriptor_create_directory_at.call(store, handle0 as i32, ptr1, vec1.len() as i32, )?;
      let _memory_view = _memory.view(&store);
      let load3 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result2 + 0)?;
      Ok(match i32::from(load3) {
        0 => Ok(()),
        1 => Err({
          let _memory_view = _memory.view(&store);
          let load4 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result2 + 1)?;
          match i32::from(load4) {
            0 => Errno::Success,
            1 => Errno::Toobig,
            2 => Errno::Access,
            3 => Errno::Addrinuse,
            4 => Errno::Addrnotavail,
            5 => Errno::Afnosupport,
            6 => Errno::Again,
            7 => Errno::Already,
            8 => Errno::Badmsg,
            9 => Errno::Busy,
            10 => Errno::Canceled,
            11 => Errno::Child,
            12 => Errno::Connaborted,
            13 => Errno::Connrefused,
            14 => Errno::Connreset,
            15 => Errno::Deadlk,
            16 => Errno::Destaddrreq,
            17 => Errno::Dom,
            18 => Errno::Dquot,
            19 => Errno::Exist,
            20 => Errno::Fault,
            21 => Errno::Fbig,
            22 => Errno::Hostunreach,
            23 => Errno::Idrm,
            24 => Errno::Ilseq,
            25 => Errno::Inprogress,
            26 => Errno::Intr,
            27 => Errno::Inval,
            28 => Errno::Io,
            29 => Errno::Isconn,
            30 => Errno::Isdir,
            31 => Errno::Loop,
            32 => Errno::Mfile,
            33 => Errno::Mlink,
            34 => Errno::Msgsize,
            35 => Errno::Multihop,
            36 => Errno::Nametoolong,
            37 => Errno::Netdown,
            38 => Errno::Netreset,
            39 => Errno::Netunreach,
            40 => Errno::Nfile,
            41 => Errno::Nobufs,
            42 => Errno::Nodev,
            43 => Errno::Noent,
            44 => Errno::Noexec,
            45 => Errno::Nolck,
            46 => Errno::Nolink,
            47 => Errno::Nomem,
            48 => Errno::Nomsg,
            49 => Errno::Noprotoopt,
            50 => Errno::Nospc,
            51 => Errno::Nosys,
            52 => Errno::Notconn,
            53 => Errno::Notdir,
            54 => Errno::Notempty,
            55 => Errno::Notrecoverable,
            56 => Errno::Notsock,
            57 => Errno::Notsup,
            58 => Errno::Notty,
            59 => Errno::Nxio,
            60 => Errno::Overflow,
            61 => Errno::Ownerdead,
            62 => Errno::Perm,
            63 => Errno::Pipe,
            64 => Errno::Proto,
            65 => Errno::Protonosupport,
            66 => Errno::Prototype,
            67 => Errno::Range,
            68 => Errno::Rofs,
            69 => Errno::Spipe,
            70 => Errno::Srch,
            71 => Errno::Stale,
            72 => Errno::Timedout,
            73 => Errno::Txtbsy,
            74 => Errno::Xdev,
            _ => return Err(invalid_variant("Errno")),
          }
        }),
        _ => return Err(invalid_variant("expected")),
      })
    }
    /// Return the attributes of a file or directory.
    /// 
    /// Note: This is similar to `fstatat` in POSIX.
    /// 
    /// Note: This was called `fd_filestat_get` in earlier versions of WASI.
    pub fn descriptor_stat_at(&self, store: &mut wasmer::Store,self_: & Descriptor,at_flags: AtFlags,path: & str,)-> Result<Result<Stat,Errno>, wasmer::RuntimeError> {
      let func_canonical_abi_realloc = &self.func_canonical_abi_realloc;
      let _memory = &self.memory;
      
      let obj0 = self_;
      let handle0 = {
        let state = self.env.as_mut(store);
        state.resource_slab0.clone(obj0.0)?;
        state.index_slab0.insert(obj0.0)
      };
      let flags1 = at_flags;
      let vec2 = path;
      let ptr2 = func_canonical_abi_realloc.call(&mut store.as_store_mut(), 0, 0, 1, vec2.len() as i32)?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store_many(ptr2, vec2.as_bytes())?;
      let result3 = self.func_descriptor_stat_at.call(store, handle0 as i32, (flags1.bits >> 0) as i32, ptr2, vec2.len() as i32, )?;
      let _memory_view = _memory.view(&store);
      let load4 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result3 + 0)?;
      Ok(match i32::from(load4) {
        0 => Ok({
          let _memory_view = _memory.view(&store);
          let load5 = unsafe { _memory_view.data_unchecked_mut() }.load::<i64>(result3 + 8)?;
          let _memory_view = _memory.view(&store);
          let load6 = unsafe { _memory_view.data_unchecked_mut() }.load::<i64>(result3 + 16)?;
          let _memory_view = _memory.view(&store);
          let load7 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result3 + 24)?;
          let _memory_view = _memory.view(&store);
          let load8 = unsafe { _memory_view.data_unchecked_mut() }.load::<i64>(result3 + 32)?;
          let _memory_view = _memory.view(&store);
          let load9 = unsafe { _memory_view.data_unchecked_mut() }.load::<i64>(result3 + 40)?;
          let _memory_view = _memory.view(&store);
          let load10 = unsafe { _memory_view.data_unchecked_mut() }.load::<i64>(result3 + 48)?;
          let _memory_view = _memory.view(&store);
          let load11 = unsafe { _memory_view.data_unchecked_mut() }.load::<i64>(result3 + 56)?;
          let _memory_view = _memory.view(&store);
          let load12 = unsafe { _memory_view.data_unchecked_mut() }.load::<i64>(result3 + 64)?;
          Stat{dev:load5 as u64, ino:load6 as u64, type_:match i32::from(load7) {
            0 => Type::Unknown,
            1 => Type::BlockDevice,
            2 => Type::CharacterDevice,
            3 => Type::Directory,
            4 => Type::Fifo,
            5 => Type::SymbolicLink,
            6 => Type::RegularFile,
            7 => Type::Socket,
            _ => return Err(invalid_variant("Type")),
          }, nlink:load8 as u64, size:load9 as u64, atim:load10 as u64, mtim:load11 as u64, ctim:load12 as u64, }
        }),
        1 => Err({
          let _memory_view = _memory.view(&store);
          let load13 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result3 + 8)?;
          match i32::from(load13) {
            0 => Errno::Success,
            1 => Errno::Toobig,
            2 => Errno::Access,
            3 => Errno::Addrinuse,
            4 => Errno::Addrnotavail,
            5 => Errno::Afnosupport,
            6 => Errno::Again,
            7 => Errno::Already,
            8 => Errno::Badmsg,
            9 => Errno::Busy,
            10 => Errno::Canceled,
            11 => Errno::Child,
            12 => Errno::Connaborted,
            13 => Errno::Connrefused,
            14 => Errno::Connreset,
            15 => Errno::Deadlk,
            16 => Errno::Destaddrreq,
            17 => Errno::Dom,
            18 => Errno::Dquot,
            19 => Errno::Exist,
            20 => Errno::Fault,
            21 => Errno::Fbig,
            22 => Errno::Hostunreach,
            23 => Errno::Idrm,
            24 => Errno::Ilseq,
            25 => Errno::Inprogress,
            26 => Errno::Intr,
            27 => Errno::Inval,
            28 => Errno::Io,
            29 => Errno::Isconn,
            30 => Errno::Isdir,
            31 => Errno::Loop,
            32 => Errno::Mfile,
            33 => Errno::Mlink,
            34 => Errno::Msgsize,
            35 => Errno::Multihop,
            36 => Errno::Nametoolong,
            37 => Errno::Netdown,
            38 => Errno::Netreset,
            39 => Errno::Netunreach,
            40 => Errno::Nfile,
            41 => Errno::Nobufs,
            42 => Errno::Nodev,
            43 => Errno::Noent,
            44 => Errno::Noexec,
            45 => Errno::Nolck,
            46 => Errno::Nolink,
            47 => Errno::Nomem,
            48 => Errno::Nomsg,
            49 => Errno::Noprotoopt,
            50 => Errno::Nospc,
            51 => Errno::Nosys,
            52 => Errno::Notconn,
            53 => Errno::Notdir,
            54 => Errno::Notempty,
            55 => Errno::Notrecoverable,
            56 => Errno::Notsock,
            57 => Errno::Notsup,
            58 => Errno::Notty,
            59 => Errno::Nxio,
            60 => Errno::Overflow,
            61 => Errno::Ownerdead,
            62 => Errno::Perm,
            63 => Errno::Pipe,
            64 => Errno::Proto,
            65 => Errno::Protonosupport,
            66 => Errno::Prototype,
            67 => Errno::Range,
            68 => Errno::Rofs,
            69 => Errno::Spipe,
            70 => Errno::Srch,
            71 => Errno::Stale,
            72 => Errno::Timedout,
            73 => Errno::Txtbsy,
            74 => Errno::Xdev,
            _ => return Err(invalid_variant("Errno")),
          }
        }),
        _ => return Err(invalid_variant("expected")),
      })
    }
    /// Adjust the timestamps of a file or directory.
    /// 
    /// Note: This is similar to `utimensat` in POSIX.
    /// 
    /// Note: This was called `path_filestat_set_times` in earlier versions of WASI.
    pub fn descriptor_set_times_at(&self, store: &mut wasmer::Store,self_: & Descriptor,at_flags: AtFlags,path: & str,atim: NewTimestamp,mtim: NewTimestamp,)-> Result<Result<(),Errno>, wasmer::RuntimeError> {
      let func_canonical_abi_realloc = &self.func_canonical_abi_realloc;
      let _memory = &self.memory;
      
      let obj0 = self_;
      let handle0 = {
        let state = self.env.as_mut(store);
        state.resource_slab0.clone(obj0.0)?;
        state.index_slab0.insert(obj0.0)
      };
      let flags1 = at_flags;
      let vec2 = path;
      let ptr2 = func_canonical_abi_realloc.call(&mut store.as_store_mut(), 0, 0, 1, vec2.len() as i32)?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store_many(ptr2, vec2.as_bytes())?;
      let (result3_0,result3_1,) = match atim {
        NewTimestamp::NoChange=> {
          let e = ();
          {
            let () = e;
            (0i32, 0i64)
          }
        }
        NewTimestamp::Now=> {
          let e = ();
          {
            let () = e;
            (1i32, 0i64)
          }
        }
        NewTimestamp::Timestamp(e) => (2i32, wit_bindgen_wasmer::rt::as_i64(e)),
      };
      let (result4_0,result4_1,) = match mtim {
        NewTimestamp::NoChange=> {
          let e = ();
          {
            let () = e;
            (0i32, 0i64)
          }
        }
        NewTimestamp::Now=> {
          let e = ();
          {
            let () = e;
            (1i32, 0i64)
          }
        }
        NewTimestamp::Timestamp(e) => (2i32, wit_bindgen_wasmer::rt::as_i64(e)),
      };
      let result5 = self.func_descriptor_set_times_at.call(store, handle0 as i32, (flags1.bits >> 0) as i32, ptr2, vec2.len() as i32, result3_0, result3_1, result4_0, result4_1, )?;
      let _memory_view = _memory.view(&store);
      let load6 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result5 + 0)?;
      Ok(match i32::from(load6) {
        0 => Ok(()),
        1 => Err({
          let _memory_view = _memory.view(&store);
          let load7 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result5 + 1)?;
          match i32::from(load7) {
            0 => Errno::Success,
            1 => Errno::Toobig,
            2 => Errno::Access,
            3 => Errno::Addrinuse,
            4 => Errno::Addrnotavail,
            5 => Errno::Afnosupport,
            6 => Errno::Again,
            7 => Errno::Already,
            8 => Errno::Badmsg,
            9 => Errno::Busy,
            10 => Errno::Canceled,
            11 => Errno::Child,
            12 => Errno::Connaborted,
            13 => Errno::Connrefused,
            14 => Errno::Connreset,
            15 => Errno::Deadlk,
            16 => Errno::Destaddrreq,
            17 => Errno::Dom,
            18 => Errno::Dquot,
            19 => Errno::Exist,
            20 => Errno::Fault,
            21 => Errno::Fbig,
            22 => Errno::Hostunreach,
            23 => Errno::Idrm,
            24 => Errno::Ilseq,
            25 => Errno::Inprogress,
            26 => Errno::Intr,
            27 => Errno::Inval,
            28 => Errno::Io,
            29 => Errno::Isconn,
            30 => Errno::Isdir,
            31 => Errno::Loop,
            32 => Errno::Mfile,
            33 => Errno::Mlink,
            34 => Errno::Msgsize,
            35 => Errno::Multihop,
            36 => Errno::Nametoolong,
            37 => Errno::Netdown,
            38 => Errno::Netreset,
            39 => Errno::Netunreach,
            40 => Errno::Nfile,
            41 => Errno::Nobufs,
            42 => Errno::Nodev,
            43 => Errno::Noent,
            44 => Errno::Noexec,
            45 => Errno::Nolck,
            46 => Errno::Nolink,
            47 => Errno::Nomem,
            48 => Errno::Nomsg,
            49 => Errno::Noprotoopt,
            50 => Errno::Nospc,
            51 => Errno::Nosys,
            52 => Errno::Notconn,
            53 => Errno::Notdir,
            54 => Errno::Notempty,
            55 => Errno::Notrecoverable,
            56 => Errno::Notsock,
            57 => Errno::Notsup,
            58 => Errno::Notty,
            59 => Errno::Nxio,
            60 => Errno::Overflow,
            61 => Errno::Ownerdead,
            62 => Errno::Perm,
            63 => Errno::Pipe,
            64 => Errno::Proto,
            65 => Errno::Protonosupport,
            66 => Errno::Prototype,
            67 => Errno::Range,
            68 => Errno::Rofs,
            69 => Errno::Spipe,
            70 => Errno::Srch,
            71 => Errno::Stale,
            72 => Errno::Timedout,
            73 => Errno::Txtbsy,
            74 => Errno::Xdev,
            _ => return Err(invalid_variant("Errno")),
          }
        }),
        _ => return Err(invalid_variant("expected")),
      })
    }
    /// Create a hard link.
    /// 
    /// Note: This is similar to `linkat` in POSIX.
    pub fn descriptor_link_at(&self, store: &mut wasmer::Store,self_: & Descriptor,old_at_flags: AtFlags,old_path: & str,new_descriptor: & Descriptor,new_path: & str,)-> Result<Result<(),Errno>, wasmer::RuntimeError> {
      let func_canonical_abi_realloc = &self.func_canonical_abi_realloc;
      let _memory = &self.memory;
      
      let obj0 = self_;
      let handle0 = {
        let state = self.env.as_mut(store);
        state.resource_slab0.clone(obj0.0)?;
        state.index_slab0.insert(obj0.0)
      };
      let flags1 = old_at_flags;
      let vec2 = old_path;
      let ptr2 = func_canonical_abi_realloc.call(&mut store.as_store_mut(), 0, 0, 1, vec2.len() as i32)?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store_many(ptr2, vec2.as_bytes())?;
      
      let obj3 = new_descriptor;
      let handle3 = {
        let state = self.env.as_mut(store);
        state.resource_slab0.clone(obj3.0)?;
        state.index_slab0.insert(obj3.0)
      };
      let vec4 = new_path;
      let ptr4 = func_canonical_abi_realloc.call(&mut store.as_store_mut(), 0, 0, 1, vec4.len() as i32)?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store_many(ptr4, vec4.as_bytes())?;
      let result5 = self.func_descriptor_link_at.call(store, handle0 as i32, (flags1.bits >> 0) as i32, ptr2, vec2.len() as i32, handle3 as i32, ptr4, vec4.len() as i32, )?;
      let _memory_view = _memory.view(&store);
      let load6 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result5 + 0)?;
      Ok(match i32::from(load6) {
        0 => Ok(()),
        1 => Err({
          let _memory_view = _memory.view(&store);
          let load7 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result5 + 1)?;
          match i32::from(load7) {
            0 => Errno::Success,
            1 => Errno::Toobig,
            2 => Errno::Access,
            3 => Errno::Addrinuse,
            4 => Errno::Addrnotavail,
            5 => Errno::Afnosupport,
            6 => Errno::Again,
            7 => Errno::Already,
            8 => Errno::Badmsg,
            9 => Errno::Busy,
            10 => Errno::Canceled,
            11 => Errno::Child,
            12 => Errno::Connaborted,
            13 => Errno::Connrefused,
            14 => Errno::Connreset,
            15 => Errno::Deadlk,
            16 => Errno::Destaddrreq,
            17 => Errno::Dom,
            18 => Errno::Dquot,
            19 => Errno::Exist,
            20 => Errno::Fault,
            21 => Errno::Fbig,
            22 => Errno::Hostunreach,
            23 => Errno::Idrm,
            24 => Errno::Ilseq,
            25 => Errno::Inprogress,
            26 => Errno::Intr,
            27 => Errno::Inval,
            28 => Errno::Io,
            29 => Errno::Isconn,
            30 => Errno::Isdir,
            31 => Errno::Loop,
            32 => Errno::Mfile,
            33 => Errno::Mlink,
            34 => Errno::Msgsize,
            35 => Errno::Multihop,
            36 => Errno::Nametoolong,
            37 => Errno::Netdown,
            38 => Errno::Netreset,
            39 => Errno::Netunreach,
            40 => Errno::Nfile,
            41 => Errno::Nobufs,
            42 => Errno::Nodev,
            43 => Errno::Noent,
            44 => Errno::Noexec,
            45 => Errno::Nolck,
            46 => Errno::Nolink,
            47 => Errno::Nomem,
            48 => Errno::Nomsg,
            49 => Errno::Noprotoopt,
            50 => Errno::Nospc,
            51 => Errno::Nosys,
            52 => Errno::Notconn,
            53 => Errno::Notdir,
            54 => Errno::Notempty,
            55 => Errno::Notrecoverable,
            56 => Errno::Notsock,
            57 => Errno::Notsup,
            58 => Errno::Notty,
            59 => Errno::Nxio,
            60 => Errno::Overflow,
            61 => Errno::Ownerdead,
            62 => Errno::Perm,
            63 => Errno::Pipe,
            64 => Errno::Proto,
            65 => Errno::Protonosupport,
            66 => Errno::Prototype,
            67 => Errno::Range,
            68 => Errno::Rofs,
            69 => Errno::Spipe,
            70 => Errno::Srch,
            71 => Errno::Stale,
            72 => Errno::Timedout,
            73 => Errno::Txtbsy,
            74 => Errno::Xdev,
            _ => return Err(invalid_variant("Errno")),
          }
        }),
        _ => return Err(invalid_variant("expected")),
      })
    }
    /// Open a file or directory.
    /// 
    /// The returned descriptor is not guaranteed to be the lowest-numbered
    /// descriptor not currently open/ it is randomized to prevent applications
    /// from depending on making assumptions about indexes, since this is
    /// error-prone in multi-threaded contexts. The returned descriptor is
    /// guaranteed to be less than 2**31.
    /// 
    /// Note: This is similar to `openat` in POSIX.
    pub fn descriptor_open_at(&self, store: &mut wasmer::Store,self_: & Descriptor,at_flags: AtFlags,path: & str,o_flags: OFlags,flags: Flags,mode: Mode,)-> Result<Result<Descriptor,Errno>, wasmer::RuntimeError> {
      let func_canonical_abi_realloc = &self.func_canonical_abi_realloc;
      let _memory = &self.memory;
      
      let obj0 = self_;
      let handle0 = {
        let state = self.env.as_mut(store);
        state.resource_slab0.clone(obj0.0)?;
        state.index_slab0.insert(obj0.0)
      };
      let flags1 = at_flags;
      let vec2 = path;
      let ptr2 = func_canonical_abi_realloc.call(&mut store.as_store_mut(), 0, 0, 1, vec2.len() as i32)?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store_many(ptr2, vec2.as_bytes())?;
      let flags3 = o_flags;
      let flags4 = flags;
      let flags5 = mode;
      let result6 = self.func_descriptor_open_at.call(store, handle0 as i32, (flags1.bits >> 0) as i32, ptr2, vec2.len() as i32, (flags3.bits >> 0) as i32, (flags4.bits >> 0) as i32, (flags5.bits >> 0) as i32, )?;
      let _memory_view = _memory.view(&store);
      let load7 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result6 + 0)?;
      Ok(match i32::from(load7) {
        0 => Ok({
          let _memory_view = _memory.view(&store);
          let load8 = unsafe { _memory_view.data_unchecked_mut() }.load::<i32>(result6 + 4)?;
          let state = self.env.as_mut(store);
          let handle9 = state.index_slab0.remove(load8 as u32)?;
          Descriptor(handle9)
        }),
        1 => Err({
          let _memory_view = _memory.view(&store);
          let load10 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result6 + 4)?;
          match i32::from(load10) {
            0 => Errno::Success,
            1 => Errno::Toobig,
            2 => Errno::Access,
            3 => Errno::Addrinuse,
            4 => Errno::Addrnotavail,
            5 => Errno::Afnosupport,
            6 => Errno::Again,
            7 => Errno::Already,
            8 => Errno::Badmsg,
            9 => Errno::Busy,
            10 => Errno::Canceled,
            11 => Errno::Child,
            12 => Errno::Connaborted,
            13 => Errno::Connrefused,
            14 => Errno::Connreset,
            15 => Errno::Deadlk,
            16 => Errno::Destaddrreq,
            17 => Errno::Dom,
            18 => Errno::Dquot,
            19 => Errno::Exist,
            20 => Errno::Fault,
            21 => Errno::Fbig,
            22 => Errno::Hostunreach,
            23 => Errno::Idrm,
            24 => Errno::Ilseq,
            25 => Errno::Inprogress,
            26 => Errno::Intr,
            27 => Errno::Inval,
            28 => Errno::Io,
            29 => Errno::Isconn,
            30 => Errno::Isdir,
            31 => Errno::Loop,
            32 => Errno::Mfile,
            33 => Errno::Mlink,
            34 => Errno::Msgsize,
            35 => Errno::Multihop,
            36 => Errno::Nametoolong,
            37 => Errno::Netdown,
            38 => Errno::Netreset,
            39 => Errno::Netunreach,
            40 => Errno::Nfile,
            41 => Errno::Nobufs,
            42 => Errno::Nodev,
            43 => Errno::Noent,
            44 => Errno::Noexec,
            45 => Errno::Nolck,
            46 => Errno::Nolink,
            47 => Errno::Nomem,
            48 => Errno::Nomsg,
            49 => Errno::Noprotoopt,
            50 => Errno::Nospc,
            51 => Errno::Nosys,
            52 => Errno::Notconn,
            53 => Errno::Notdir,
            54 => Errno::Notempty,
            55 => Errno::Notrecoverable,
            56 => Errno::Notsock,
            57 => Errno::Notsup,
            58 => Errno::Notty,
            59 => Errno::Nxio,
            60 => Errno::Overflow,
            61 => Errno::Ownerdead,
            62 => Errno::Perm,
            63 => Errno::Pipe,
            64 => Errno::Proto,
            65 => Errno::Protonosupport,
            66 => Errno::Prototype,
            67 => Errno::Range,
            68 => Errno::Rofs,
            69 => Errno::Spipe,
            70 => Errno::Srch,
            71 => Errno::Stale,
            72 => Errno::Timedout,
            73 => Errno::Txtbsy,
            74 => Errno::Xdev,
            _ => return Err(invalid_variant("Errno")),
          }
        }),
        _ => return Err(invalid_variant("expected")),
      })
    }
    /// Read the contents of a symbolic link.
    /// 
    /// Note: This is similar to `readlinkat` in POSIX.
    pub fn descriptor_readlink_at(&self, store: &mut wasmer::Store,self_: & Descriptor,path: & str,)-> Result<Result<String,Errno>, wasmer::RuntimeError> {
      let func_canonical_abi_free = &self.func_canonical_abi_free;
      let func_canonical_abi_realloc = &self.func_canonical_abi_realloc;
      let _memory = &self.memory;
      
      let obj0 = self_;
      let handle0 = {
        let state = self.env.as_mut(store);
        state.resource_slab0.clone(obj0.0)?;
        state.index_slab0.insert(obj0.0)
      };
      let vec1 = path;
      let ptr1 = func_canonical_abi_realloc.call(&mut store.as_store_mut(), 0, 0, 1, vec1.len() as i32)?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store_many(ptr1, vec1.as_bytes())?;
      let result2 = self.func_descriptor_readlink_at.call(store, handle0 as i32, ptr1, vec1.len() as i32, )?;
      let _memory_view = _memory.view(&store);
      let load3 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result2 + 0)?;
      Ok(match i32::from(load3) {
        0 => Ok({
          let _memory_view = _memory.view(&store);
          let load4 = unsafe { _memory_view.data_unchecked_mut() }.load::<i32>(result2 + 4)?;
          let _memory_view = _memory.view(&store);
          let load5 = unsafe { _memory_view.data_unchecked_mut() }.load::<i32>(result2 + 8)?;
          let ptr6 = load4;
          let len6 = load5;
          
          let data6 = copy_slice(
          store,
          _memory,
          func_canonical_abi_free,
          ptr6, len6, 1,
          )?;
          String::from_utf8(data6)
          .map_err(|_| wasmer::RuntimeError::new("invalid utf-8"))?
        }),
        1 => Err({
          let _memory_view = _memory.view(&store);
          let load7 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result2 + 4)?;
          match i32::from(load7) {
            0 => Errno::Success,
            1 => Errno::Toobig,
            2 => Errno::Access,
            3 => Errno::Addrinuse,
            4 => Errno::Addrnotavail,
            5 => Errno::Afnosupport,
            6 => Errno::Again,
            7 => Errno::Already,
            8 => Errno::Badmsg,
            9 => Errno::Busy,
            10 => Errno::Canceled,
            11 => Errno::Child,
            12 => Errno::Connaborted,
            13 => Errno::Connrefused,
            14 => Errno::Connreset,
            15 => Errno::Deadlk,
            16 => Errno::Destaddrreq,
            17 => Errno::Dom,
            18 => Errno::Dquot,
            19 => Errno::Exist,
            20 => Errno::Fault,
            21 => Errno::Fbig,
            22 => Errno::Hostunreach,
            23 => Errno::Idrm,
            24 => Errno::Ilseq,
            25 => Errno::Inprogress,
            26 => Errno::Intr,
            27 => Errno::Inval,
            28 => Errno::Io,
            29 => Errno::Isconn,
            30 => Errno::Isdir,
            31 => Errno::Loop,
            32 => Errno::Mfile,
            33 => Errno::Mlink,
            34 => Errno::Msgsize,
            35 => Errno::Multihop,
            36 => Errno::Nametoolong,
            37 => Errno::Netdown,
            38 => Errno::Netreset,
            39 => Errno::Netunreach,
            40 => Errno::Nfile,
            41 => Errno::Nobufs,
            42 => Errno::Nodev,
            43 => Errno::Noent,
            44 => Errno::Noexec,
            45 => Errno::Nolck,
            46 => Errno::Nolink,
            47 => Errno::Nomem,
            48 => Errno::Nomsg,
            49 => Errno::Noprotoopt,
            50 => Errno::Nospc,
            51 => Errno::Nosys,
            52 => Errno::Notconn,
            53 => Errno::Notdir,
            54 => Errno::Notempty,
            55 => Errno::Notrecoverable,
            56 => Errno::Notsock,
            57 => Errno::Notsup,
            58 => Errno::Notty,
            59 => Errno::Nxio,
            60 => Errno::Overflow,
            61 => Errno::Ownerdead,
            62 => Errno::Perm,
            63 => Errno::Pipe,
            64 => Errno::Proto,
            65 => Errno::Protonosupport,
            66 => Errno::Prototype,
            67 => Errno::Range,
            68 => Errno::Rofs,
            69 => Errno::Spipe,
            70 => Errno::Srch,
            71 => Errno::Stale,
            72 => Errno::Timedout,
            73 => Errno::Txtbsy,
            74 => Errno::Xdev,
            _ => return Err(invalid_variant("Errno")),
          }
        }),
        _ => return Err(invalid_variant("expected")),
      })
    }
    /// Remove a directory.
    /// 
    /// Return `errno::notempty` if the directory is not empty.
    /// 
    /// Note: This is similar to `unlinkat(fd, path, AT_REMOVEDIR)` in POSIX.
    pub fn descriptor_remove_directory_at(&self, store: &mut wasmer::Store,self_: & Descriptor,path: & str,)-> Result<Result<(),Errno>, wasmer::RuntimeError> {
      let func_canonical_abi_realloc = &self.func_canonical_abi_realloc;
      let _memory = &self.memory;
      
      let obj0 = self_;
      let handle0 = {
        let state = self.env.as_mut(store);
        state.resource_slab0.clone(obj0.0)?;
        state.index_slab0.insert(obj0.0)
      };
      let vec1 = path;
      let ptr1 = func_canonical_abi_realloc.call(&mut store.as_store_mut(), 0, 0, 1, vec1.len() as i32)?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store_many(ptr1, vec1.as_bytes())?;
      let result2 = self.func_descriptor_remove_directory_at.call(store, handle0 as i32, ptr1, vec1.len() as i32, )?;
      let _memory_view = _memory.view(&store);
      let load3 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result2 + 0)?;
      Ok(match i32::from(load3) {
        0 => Ok(()),
        1 => Err({
          let _memory_view = _memory.view(&store);
          let load4 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result2 + 1)?;
          match i32::from(load4) {
            0 => Errno::Success,
            1 => Errno::Toobig,
            2 => Errno::Access,
            3 => Errno::Addrinuse,
            4 => Errno::Addrnotavail,
            5 => Errno::Afnosupport,
            6 => Errno::Again,
            7 => Errno::Already,
            8 => Errno::Badmsg,
            9 => Errno::Busy,
            10 => Errno::Canceled,
            11 => Errno::Child,
            12 => Errno::Connaborted,
            13 => Errno::Connrefused,
            14 => Errno::Connreset,
            15 => Errno::Deadlk,
            16 => Errno::Destaddrreq,
            17 => Errno::Dom,
            18 => Errno::Dquot,
            19 => Errno::Exist,
            20 => Errno::Fault,
            21 => Errno::Fbig,
            22 => Errno::Hostunreach,
            23 => Errno::Idrm,
            24 => Errno::Ilseq,
            25 => Errno::Inprogress,
            26 => Errno::Intr,
            27 => Errno::Inval,
            28 => Errno::Io,
            29 => Errno::Isconn,
            30 => Errno::Isdir,
            31 => Errno::Loop,
            32 => Errno::Mfile,
            33 => Errno::Mlink,
            34 => Errno::Msgsize,
            35 => Errno::Multihop,
            36 => Errno::Nametoolong,
            37 => Errno::Netdown,
            38 => Errno::Netreset,
            39 => Errno::Netunreach,
            40 => Errno::Nfile,
            41 => Errno::Nobufs,
            42 => Errno::Nodev,
            43 => Errno::Noent,
            44 => Errno::Noexec,
            45 => Errno::Nolck,
            46 => Errno::Nolink,
            47 => Errno::Nomem,
            48 => Errno::Nomsg,
            49 => Errno::Noprotoopt,
            50 => Errno::Nospc,
            51 => Errno::Nosys,
            52 => Errno::Notconn,
            53 => Errno::Notdir,
            54 => Errno::Notempty,
            55 => Errno::Notrecoverable,
            56 => Errno::Notsock,
            57 => Errno::Notsup,
            58 => Errno::Notty,
            59 => Errno::Nxio,
            60 => Errno::Overflow,
            61 => Errno::Ownerdead,
            62 => Errno::Perm,
            63 => Errno::Pipe,
            64 => Errno::Proto,
            65 => Errno::Protonosupport,
            66 => Errno::Prototype,
            67 => Errno::Range,
            68 => Errno::Rofs,
            69 => Errno::Spipe,
            70 => Errno::Srch,
            71 => Errno::Stale,
            72 => Errno::Timedout,
            73 => Errno::Txtbsy,
            74 => Errno::Xdev,
            _ => return Err(invalid_variant("Errno")),
          }
        }),
        _ => return Err(invalid_variant("expected")),
      })
    }
    /// Rename a filesystem object.
    /// 
    /// Note: This is similar to `renameat` in POSIX.
    pub fn descriptor_rename_at(&self, store: &mut wasmer::Store,self_: & Descriptor,old_path: & str,new_descriptor: & Descriptor,new_path: & str,)-> Result<Result<(),Errno>, wasmer::RuntimeError> {
      let func_canonical_abi_realloc = &self.func_canonical_abi_realloc;
      let _memory = &self.memory;
      
      let obj0 = self_;
      let handle0 = {
        let state = self.env.as_mut(store);
        state.resource_slab0.clone(obj0.0)?;
        state.index_slab0.insert(obj0.0)
      };
      let vec1 = old_path;
      let ptr1 = func_canonical_abi_realloc.call(&mut store.as_store_mut(), 0, 0, 1, vec1.len() as i32)?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store_many(ptr1, vec1.as_bytes())?;
      
      let obj2 = new_descriptor;
      let handle2 = {
        let state = self.env.as_mut(store);
        state.resource_slab0.clone(obj2.0)?;
        state.index_slab0.insert(obj2.0)
      };
      let vec3 = new_path;
      let ptr3 = func_canonical_abi_realloc.call(&mut store.as_store_mut(), 0, 0, 1, vec3.len() as i32)?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store_many(ptr3, vec3.as_bytes())?;
      let result4 = self.func_descriptor_rename_at.call(store, handle0 as i32, ptr1, vec1.len() as i32, handle2 as i32, ptr3, vec3.len() as i32, )?;
      let _memory_view = _memory.view(&store);
      let load5 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result4 + 0)?;
      Ok(match i32::from(load5) {
        0 => Ok(()),
        1 => Err({
          let _memory_view = _memory.view(&store);
          let load6 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result4 + 1)?;
          match i32::from(load6) {
            0 => Errno::Success,
            1 => Errno::Toobig,
            2 => Errno::Access,
            3 => Errno::Addrinuse,
            4 => Errno::Addrnotavail,
            5 => Errno::Afnosupport,
            6 => Errno::Again,
            7 => Errno::Already,
            8 => Errno::Badmsg,
            9 => Errno::Busy,
            10 => Errno::Canceled,
            11 => Errno::Child,
            12 => Errno::Connaborted,
            13 => Errno::Connrefused,
            14 => Errno::Connreset,
            15 => Errno::Deadlk,
            16 => Errno::Destaddrreq,
            17 => Errno::Dom,
            18 => Errno::Dquot,
            19 => Errno::Exist,
            20 => Errno::Fault,
            21 => Errno::Fbig,
            22 => Errno::Hostunreach,
            23 => Errno::Idrm,
            24 => Errno::Ilseq,
            25 => Errno::Inprogress,
            26 => Errno::Intr,
            27 => Errno::Inval,
            28 => Errno::Io,
            29 => Errno::Isconn,
            30 => Errno::Isdir,
            31 => Errno::Loop,
            32 => Errno::Mfile,
            33 => Errno::Mlink,
            34 => Errno::Msgsize,
            35 => Errno::Multihop,
            36 => Errno::Nametoolong,
            37 => Errno::Netdown,
            38 => Errno::Netreset,
            39 => Errno::Netunreach,
            40 => Errno::Nfile,
            41 => Errno::Nobufs,
            42 => Errno::Nodev,
            43 => Errno::Noent,
            44 => Errno::Noexec,
            45 => Errno::Nolck,
            46 => Errno::Nolink,
            47 => Errno::Nomem,
            48 => Errno::Nomsg,
            49 => Errno::Noprotoopt,
            50 => Errno::Nospc,
            51 => Errno::Nosys,
            52 => Errno::Notconn,
            53 => Errno::Notdir,
            54 => Errno::Notempty,
            55 => Errno::Notrecoverable,
            56 => Errno::Notsock,
            57 => Errno::Notsup,
            58 => Errno::Notty,
            59 => Errno::Nxio,
            60 => Errno::Overflow,
            61 => Errno::Ownerdead,
            62 => Errno::Perm,
            63 => Errno::Pipe,
            64 => Errno::Proto,
            65 => Errno::Protonosupport,
            66 => Errno::Prototype,
            67 => Errno::Range,
            68 => Errno::Rofs,
            69 => Errno::Spipe,
            70 => Errno::Srch,
            71 => Errno::Stale,
            72 => Errno::Timedout,
            73 => Errno::Txtbsy,
            74 => Errno::Xdev,
            _ => return Err(invalid_variant("Errno")),
          }
        }),
        _ => return Err(invalid_variant("expected")),
      })
    }
    /// Create a symbolic link.
    /// 
    /// Note: This is similar to `symlinkat` in POSIX.
    pub fn descriptor_symlink_at(&self, store: &mut wasmer::Store,self_: & Descriptor,old_path: & str,new_path: & str,)-> Result<Result<(),Errno>, wasmer::RuntimeError> {
      let func_canonical_abi_realloc = &self.func_canonical_abi_realloc;
      let _memory = &self.memory;
      
      let obj0 = self_;
      let handle0 = {
        let state = self.env.as_mut(store);
        state.resource_slab0.clone(obj0.0)?;
        state.index_slab0.insert(obj0.0)
      };
      let vec1 = old_path;
      let ptr1 = func_canonical_abi_realloc.call(&mut store.as_store_mut(), 0, 0, 1, vec1.len() as i32)?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store_many(ptr1, vec1.as_bytes())?;
      let vec2 = new_path;
      let ptr2 = func_canonical_abi_realloc.call(&mut store.as_store_mut(), 0, 0, 1, vec2.len() as i32)?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store_many(ptr2, vec2.as_bytes())?;
      let result3 = self.func_descriptor_symlink_at.call(store, handle0 as i32, ptr1, vec1.len() as i32, ptr2, vec2.len() as i32, )?;
      let _memory_view = _memory.view(&store);
      let load4 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result3 + 0)?;
      Ok(match i32::from(load4) {
        0 => Ok(()),
        1 => Err({
          let _memory_view = _memory.view(&store);
          let load5 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result3 + 1)?;
          match i32::from(load5) {
            0 => Errno::Success,
            1 => Errno::Toobig,
            2 => Errno::Access,
            3 => Errno::Addrinuse,
            4 => Errno::Addrnotavail,
            5 => Errno::Afnosupport,
            6 => Errno::Again,
            7 => Errno::Already,
            8 => Errno::Badmsg,
            9 => Errno::Busy,
            10 => Errno::Canceled,
            11 => Errno::Child,
            12 => Errno::Connaborted,
            13 => Errno::Connrefused,
            14 => Errno::Connreset,
            15 => Errno::Deadlk,
            16 => Errno::Destaddrreq,
            17 => Errno::Dom,
            18 => Errno::Dquot,
            19 => Errno::Exist,
            20 => Errno::Fault,
            21 => Errno::Fbig,
            22 => Errno::Hostunreach,
            23 => Errno::Idrm,
            24 => Errno::Ilseq,
            25 => Errno::Inprogress,
            26 => Errno::Intr,
            27 => Errno::Inval,
            28 => Errno::Io,
            29 => Errno::Isconn,
            30 => Errno::Isdir,
            31 => Errno::Loop,
            32 => Errno::Mfile,
            33 => Errno::Mlink,
            34 => Errno::Msgsize,
            35 => Errno::Multihop,
            36 => Errno::Nametoolong,
            37 => Errno::Netdown,
            38 => Errno::Netreset,
            39 => Errno::Netunreach,
            40 => Errno::Nfile,
            41 => Errno::Nobufs,
            42 => Errno::Nodev,
            43 => Errno::Noent,
            44 => Errno::Noexec,
            45 => Errno::Nolck,
            46 => Errno::Nolink,
            47 => Errno::Nomem,
            48 => Errno::Nomsg,
            49 => Errno::Noprotoopt,
            50 => Errno::Nospc,
            51 => Errno::Nosys,
            52 => Errno::Notconn,
            53 => Errno::Notdir,
            54 => Errno::Notempty,
            55 => Errno::Notrecoverable,
            56 => Errno::Notsock,
            57 => Errno::Notsup,
            58 => Errno::Notty,
            59 => Errno::Nxio,
            60 => Errno::Overflow,
            61 => Errno::Ownerdead,
            62 => Errno::Perm,
            63 => Errno::Pipe,
            64 => Errno::Proto,
            65 => Errno::Protonosupport,
            66 => Errno::Prototype,
            67 => Errno::Range,
            68 => Errno::Rofs,
            69 => Errno::Spipe,
            70 => Errno::Srch,
            71 => Errno::Stale,
            72 => Errno::Timedout,
            73 => Errno::Txtbsy,
            74 => Errno::Xdev,
            _ => return Err(invalid_variant("Errno")),
          }
        }),
        _ => return Err(invalid_variant("expected")),
      })
    }
    /// Unlink a filesystem object that is not a directory.
    /// 
    /// Return `errno::isdir` if the path refers to a directory.
    /// Note: This is similar to `unlinkat(fd, path, 0)` in POSIX.
    pub fn descriptor_unlink_file_at(&self, store: &mut wasmer::Store,self_: & Descriptor,path: & str,)-> Result<Result<(),Errno>, wasmer::RuntimeError> {
      let func_canonical_abi_realloc = &self.func_canonical_abi_realloc;
      let _memory = &self.memory;
      
      let obj0 = self_;
      let handle0 = {
        let state = self.env.as_mut(store);
        state.resource_slab0.clone(obj0.0)?;
        state.index_slab0.insert(obj0.0)
      };
      let vec1 = path;
      let ptr1 = func_canonical_abi_realloc.call(&mut store.as_store_mut(), 0, 0, 1, vec1.len() as i32)?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store_many(ptr1, vec1.as_bytes())?;
      let result2 = self.func_descriptor_unlink_file_at.call(store, handle0 as i32, ptr1, vec1.len() as i32, )?;
      let _memory_view = _memory.view(&store);
      let load3 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result2 + 0)?;
      Ok(match i32::from(load3) {
        0 => Ok(()),
        1 => Err({
          let _memory_view = _memory.view(&store);
          let load4 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result2 + 1)?;
          match i32::from(load4) {
            0 => Errno::Success,
            1 => Errno::Toobig,
            2 => Errno::Access,
            3 => Errno::Addrinuse,
            4 => Errno::Addrnotavail,
            5 => Errno::Afnosupport,
            6 => Errno::Again,
            7 => Errno::Already,
            8 => Errno::Badmsg,
            9 => Errno::Busy,
            10 => Errno::Canceled,
            11 => Errno::Child,
            12 => Errno::Connaborted,
            13 => Errno::Connrefused,
            14 => Errno::Connreset,
            15 => Errno::Deadlk,
            16 => Errno::Destaddrreq,
            17 => Errno::Dom,
            18 => Errno::Dquot,
            19 => Errno::Exist,
            20 => Errno::Fault,
            21 => Errno::Fbig,
            22 => Errno::Hostunreach,
            23 => Errno::Idrm,
            24 => Errno::Ilseq,
            25 => Errno::Inprogress,
            26 => Errno::Intr,
            27 => Errno::Inval,
            28 => Errno::Io,
            29 => Errno::Isconn,
            30 => Errno::Isdir,
            31 => Errno::Loop,
            32 => Errno::Mfile,
            33 => Errno::Mlink,
            34 => Errno::Msgsize,
            35 => Errno::Multihop,
            36 => Errno::Nametoolong,
            37 => Errno::Netdown,
            38 => Errno::Netreset,
            39 => Errno::Netunreach,
            40 => Errno::Nfile,
            41 => Errno::Nobufs,
            42 => Errno::Nodev,
            43 => Errno::Noent,
            44 => Errno::Noexec,
            45 => Errno::Nolck,
            46 => Errno::Nolink,
            47 => Errno::Nomem,
            48 => Errno::Nomsg,
            49 => Errno::Noprotoopt,
            50 => Errno::Nospc,
            51 => Errno::Nosys,
            52 => Errno::Notconn,
            53 => Errno::Notdir,
            54 => Errno::Notempty,
            55 => Errno::Notrecoverable,
            56 => Errno::Notsock,
            57 => Errno::Notsup,
            58 => Errno::Notty,
            59 => Errno::Nxio,
            60 => Errno::Overflow,
            61 => Errno::Ownerdead,
            62 => Errno::Perm,
            63 => Errno::Pipe,
            64 => Errno::Proto,
            65 => Errno::Protonosupport,
            66 => Errno::Prototype,
            67 => Errno::Range,
            68 => Errno::Rofs,
            69 => Errno::Spipe,
            70 => Errno::Srch,
            71 => Errno::Stale,
            72 => Errno::Timedout,
            73 => Errno::Txtbsy,
            74 => Errno::Xdev,
            _ => return Err(invalid_variant("Errno")),
          }
        }),
        _ => return Err(invalid_variant("expected")),
      })
    }
    /// Change the permissions of a filesystem object that is not a directory.
    /// 
    /// Note that the ultimate meanings of these permissions is
    /// filesystem-specific.
    /// 
    /// Note: This is similar to `fchmodat` in POSIX.
    pub fn descriptor_change_file_permissions_at(&self, store: &mut wasmer::Store,self_: & Descriptor,at_flags: AtFlags,path: & str,mode: Mode,)-> Result<Result<(),Errno>, wasmer::RuntimeError> {
      let func_canonical_abi_realloc = &self.func_canonical_abi_realloc;
      let _memory = &self.memory;
      
      let obj0 = self_;
      let handle0 = {
        let state = self.env.as_mut(store);
        state.resource_slab0.clone(obj0.0)?;
        state.index_slab0.insert(obj0.0)
      };
      let flags1 = at_flags;
      let vec2 = path;
      let ptr2 = func_canonical_abi_realloc.call(&mut store.as_store_mut(), 0, 0, 1, vec2.len() as i32)?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store_many(ptr2, vec2.as_bytes())?;
      let flags3 = mode;
      let result4 = self.func_descriptor_change_file_permissions_at.call(store, handle0 as i32, (flags1.bits >> 0) as i32, ptr2, vec2.len() as i32, (flags3.bits >> 0) as i32, )?;
      let _memory_view = _memory.view(&store);
      let load5 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result4 + 0)?;
      Ok(match i32::from(load5) {
        0 => Ok(()),
        1 => Err({
          let _memory_view = _memory.view(&store);
          let load6 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result4 + 1)?;
          match i32::from(load6) {
            0 => Errno::Success,
            1 => Errno::Toobig,
            2 => Errno::Access,
            3 => Errno::Addrinuse,
            4 => Errno::Addrnotavail,
            5 => Errno::Afnosupport,
            6 => Errno::Again,
            7 => Errno::Already,
            8 => Errno::Badmsg,
            9 => Errno::Busy,
            10 => Errno::Canceled,
            11 => Errno::Child,
            12 => Errno::Connaborted,
            13 => Errno::Connrefused,
            14 => Errno::Connreset,
            15 => Errno::Deadlk,
            16 => Errno::Destaddrreq,
            17 => Errno::Dom,
            18 => Errno::Dquot,
            19 => Errno::Exist,
            20 => Errno::Fault,
            21 => Errno::Fbig,
            22 => Errno::Hostunreach,
            23 => Errno::Idrm,
            24 => Errno::Ilseq,
            25 => Errno::Inprogress,
            26 => Errno::Intr,
            27 => Errno::Inval,
            28 => Errno::Io,
            29 => Errno::Isconn,
            30 => Errno::Isdir,
            31 => Errno::Loop,
            32 => Errno::Mfile,
            33 => Errno::Mlink,
            34 => Errno::Msgsize,
            35 => Errno::Multihop,
            36 => Errno::Nametoolong,
            37 => Errno::Netdown,
            38 => Errno::Netreset,
            39 => Errno::Netunreach,
            40 => Errno::Nfile,
            41 => Errno::Nobufs,
            42 => Errno::Nodev,
            43 => Errno::Noent,
            44 => Errno::Noexec,
            45 => Errno::Nolck,
            46 => Errno::Nolink,
            47 => Errno::Nomem,
            48 => Errno::Nomsg,
            49 => Errno::Noprotoopt,
            50 => Errno::Nospc,
            51 => Errno::Nosys,
            52 => Errno::Notconn,
            53 => Errno::Notdir,
            54 => Errno::Notempty,
            55 => Errno::Notrecoverable,
            56 => Errno::Notsock,
            57 => Errno::Notsup,
            58 => Errno::Notty,
            59 => Errno::Nxio,
            60 => Errno::Overflow,
            61 => Errno::Ownerdead,
            62 => Errno::Perm,
            63 => Errno::Pipe,
            64 => Errno::Proto,
            65 => Errno::Protonosupport,
            66 => Errno::Prototype,
            67 => Errno::Range,
            68 => Errno::Rofs,
            69 => Errno::Spipe,
            70 => Errno::Srch,
            71 => Errno::Stale,
            72 => Errno::Timedout,
            73 => Errno::Txtbsy,
            74 => Errno::Xdev,
            _ => return Err(invalid_variant("Errno")),
          }
        }),
        _ => return Err(invalid_variant("expected")),
      })
    }
    /// Change the permissions of a directory.
    /// 
    /// Note that the ultimate meanings of these permissions is
    /// filesystem-specific.
    /// 
    /// Unlike in POSIX, the `executable` flag is not reinterpreted as a "search"
    /// flag. `read` on a directory implies readability and searchability, and
    /// `execute` is not valid for directories.
    /// 
    /// Note: This is similar to `fchmodat` in POSIX.
    pub fn descriptor_change_directory_permissions_at(&self, store: &mut wasmer::Store,self_: & Descriptor,at_flags: AtFlags,path: & str,mode: Mode,)-> Result<Result<(),Errno>, wasmer::RuntimeError> {
      let func_canonical_abi_realloc = &self.func_canonical_abi_realloc;
      let _memory = &self.memory;
      
      let obj0 = self_;
      let handle0 = {
        let state = self.env.as_mut(store);
        state.resource_slab0.clone(obj0.0)?;
        state.index_slab0.insert(obj0.0)
      };
      let flags1 = at_flags;
      let vec2 = path;
      let ptr2 = func_canonical_abi_realloc.call(&mut store.as_store_mut(), 0, 0, 1, vec2.len() as i32)?;
      let _memory_view = _memory.view(&store);
      unsafe { _memory_view.data_unchecked_mut() }.store_many(ptr2, vec2.as_bytes())?;
      let flags3 = mode;
      let result4 = self.func_descriptor_change_directory_permissions_at.call(store, handle0 as i32, (flags1.bits >> 0) as i32, ptr2, vec2.len() as i32, (flags3.bits >> 0) as i32, )?;
      let _memory_view = _memory.view(&store);
      let load5 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result4 + 0)?;
      Ok(match i32::from(load5) {
        0 => Ok(()),
        1 => Err({
          let _memory_view = _memory.view(&store);
          let load6 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result4 + 1)?;
          match i32::from(load6) {
            0 => Errno::Success,
            1 => Errno::Toobig,
            2 => Errno::Access,
            3 => Errno::Addrinuse,
            4 => Errno::Addrnotavail,
            5 => Errno::Afnosupport,
            6 => Errno::Again,
            7 => Errno::Already,
            8 => Errno::Badmsg,
            9 => Errno::Busy,
            10 => Errno::Canceled,
            11 => Errno::Child,
            12 => Errno::Connaborted,
            13 => Errno::Connrefused,
            14 => Errno::Connreset,
            15 => Errno::Deadlk,
            16 => Errno::Destaddrreq,
            17 => Errno::Dom,
            18 => Errno::Dquot,
            19 => Errno::Exist,
            20 => Errno::Fault,
            21 => Errno::Fbig,
            22 => Errno::Hostunreach,
            23 => Errno::Idrm,
            24 => Errno::Ilseq,
            25 => Errno::Inprogress,
            26 => Errno::Intr,
            27 => Errno::Inval,
            28 => Errno::Io,
            29 => Errno::Isconn,
            30 => Errno::Isdir,
            31 => Errno::Loop,
            32 => Errno::Mfile,
            33 => Errno::Mlink,
            34 => Errno::Msgsize,
            35 => Errno::Multihop,
            36 => Errno::Nametoolong,
            37 => Errno::Netdown,
            38 => Errno::Netreset,
            39 => Errno::Netunreach,
            40 => Errno::Nfile,
            41 => Errno::Nobufs,
            42 => Errno::Nodev,
            43 => Errno::Noent,
            44 => Errno::Noexec,
            45 => Errno::Nolck,
            46 => Errno::Nolink,
            47 => Errno::Nomem,
            48 => Errno::Nomsg,
            49 => Errno::Noprotoopt,
            50 => Errno::Nospc,
            51 => Errno::Nosys,
            52 => Errno::Notconn,
            53 => Errno::Notdir,
            54 => Errno::Notempty,
            55 => Errno::Notrecoverable,
            56 => Errno::Notsock,
            57 => Errno::Notsup,
            58 => Errno::Notty,
            59 => Errno::Nxio,
            60 => Errno::Overflow,
            61 => Errno::Ownerdead,
            62 => Errno::Perm,
            63 => Errno::Pipe,
            64 => Errno::Proto,
            65 => Errno::Protonosupport,
            66 => Errno::Prototype,
            67 => Errno::Range,
            68 => Errno::Rofs,
            69 => Errno::Spipe,
            70 => Errno::Srch,
            71 => Errno::Stale,
            72 => Errno::Timedout,
            73 => Errno::Txtbsy,
            74 => Errno::Xdev,
            _ => return Err(invalid_variant("Errno")),
          }
        }),
        _ => return Err(invalid_variant("expected")),
      })
    }
    
    /// Drops the host-owned handle to the resource
    /// specified.
    ///
    /// Note that this may execute the WebAssembly-defined
    /// destructor for this type. This also may not run
    /// the destructor if there are still other references
    /// to this type.
    pub fn drop_descriptor(
    &self,
    store: &mut wasmer::Store,
    val: Descriptor,
    ) -> Result<(), wasmer::RuntimeError> {
      let state = self.env.as_mut(store);
      let wasm = match state.resource_slab0.drop(val.0) {
        Some(val) => val,
        None => return Ok(()),
      };
      let dtor0 = state.dtor0.get().unwrap().clone();
      dtor0.call(store, wasm)?;
      Ok(())
    }
  }
  use wit_bindgen_wasmer::once_cell::unsync::OnceCell;
  #[allow(unused_imports)]
  use wasmer::AsStoreMut as _;
  #[allow(unused_imports)]
  use wasmer::AsStoreRef as _;
  use wit_bindgen_wasmer::rt::RawMem;
  use wit_bindgen_wasmer::rt::invalid_variant;
  use wit_bindgen_wasmer::rt::validate_flags;
  use wit_bindgen_wasmer::rt::copy_slice;
}
