use crate::wasi::bindings::*;

impl Rights {
    pub const fn all_socket() -> Self {
        Self::from_bits_truncate(
            Self::FD_FDSTAT_SET_FLAGS.bits()
                | Self::FD_FILESTAT_GET.bits()
                | Self::FD_READ.bits()
                | Self::FD_WRITE.bits()
                | Self::POLL_FD_READWRITE.bits()
                | Self::SOCK_SHUTDOWN.bits()
                | Self::SOCK_CONNECT.bits()
                | Self::SOCK_LISTEN.bits()
                | Self::SOCK_BIND.bits()
                | Self::SOCK_ACCEPT.bits()
                | Self::SOCK_RECV.bits()
                | Self::SOCK_SEND.bits()
                | Self::SOCK_ADDR_LOCAL.bits()
                | Self::SOCK_ADDR_REMOTE.bits()
                | Self::SOCK_RECV_FROM.bits()
                | Self::SOCK_SEND_TO.bits(),
        )
    }

    /// expects a single right, returns None if out of bounds or > 1 bit set
    pub fn to_str(self) -> Option<&'static str> {
        Some(match self {
            Rights::FD_DATASYNC => "Rights::FD_DATASYNC",
            Rights::FD_READ => "Rights::FD_READ",
            Rights::FD_SEEK => "Rights::FD_SEEK",
            Rights::FD_FDSTAT_SET_FLAGS => "Rights::FD_FDSTAT_SET_FLAGS",
            Rights::FD_SYNC => "Rights::FD_SYNC",
            Rights::FD_TELL => "Rights::FD_TELL",
            Rights::FD_WRITE => "Rights::FD_WRITE",
            Rights::FD_ADVISE => "Rights::FD_ADVISE",
            Rights::FD_ALLOCATE => "Rights::FD_ALLOCATE",
            Rights::PATH_CREATE_DIRECTORY => "Rights::PATH_CREATE_DIRECTORY",
            Rights::PATH_CREATE_FILE => "Rights::PATH_CREATE_FILE",
            Rights::PATH_LINK_SOURCE => "Rights::PATH_LINK_SOURCE",
            Rights::PATH_LINK_TARGET => "Rights::PATH_LINK_TARGET",
            Rights::PATH_OPEN => "Rights::PATH_OPEN",
            Rights::FD_READDIR => "Rights::FD_READDIR",
            Rights::PATH_READLINK => "Rights::PATH_READLINK",
            Rights::PATH_RENAME_SOURCE => "Rights::PATH_RENAME_SOURCE",
            Rights::PATH_RENAME_TARGET => "Rights::PATH_RENAME_TARGET",
            Rights::PATH_FILESTAT_GET => "Rights::PATH_FILESTAT_GET",
            Rights::PATH_FILESTAT_SET_SIZE => "Rights::PATH_FILESTAT_SET_SIZE",
            Rights::PATH_FILESTAT_SET_TIMES => "Rights::PATH_FILESTAT_SET_TIMES",
            Rights::FD_FILESTAT_GET => "Rights::FD_FILESTAT_GET",
            Rights::FD_FILESTAT_SET_SIZE => "Rights::FD_FILESTAT_SET_SIZE",
            Rights::FD_FILESTAT_SET_TIMES => "Rights::FD_FILESTAT_SET_TIMES",
            Rights::PATH_SYMLINK => "Rights::PATH_SYMLINK",
            Rights::PATH_REMOVE_DIRECTORY => "Rights::PATH_REMOVE_DIRECTORY",
            Rights::PATH_UNLINK_FILE => "Rights::PATH_UNLINK_FILE",
            Rights::POLL_FD_READWRITE => "Rights::POLL_FD_READWRITE",
            Rights::SOCK_SHUTDOWN => "Rights::SOCK_SHUTDOWN",
            Rights::SOCK_ACCEPT => "Rights::SOCK_ACCEPT",
            Rights::SOCK_CONNECT => "Rights::SOCK_CONNECT",
            Rights::SOCK_LISTEN => "Rights::SOCK_LISTEN",
            Rights::SOCK_BIND => "Rights::SOCK_BIND",
            Rights::SOCK_RECV => "Rights::SOCK_RECV",
            Rights::SOCK_SEND => "Rights::SOCK_SEND",
            Rights::SOCK_ADDR_LOCAL => "Rights::SOCK_ADDR_LOCAL",
            Rights::SOCK_ADDR_REMOTE => "Rights::SOCK_ADDR_REMOTE",
            Rights::SOCK_RECV_FROM => "Rights::SOCK_RECV_FROM",
            Rights::SOCK_SEND_TO => "Rights::SOCK_SEND_TO",
            _ => return None,
        })
    }
}

impl Default for Filestat {
    fn default() -> Self {
        Self {
            st_dev: Default::default(),
            st_ino: Default::default(),
            st_filetype: Filetype::Unknown,
            st_nlink: 1,
            st_size: Default::default(),
            st_atim: Default::default(),
            st_mtim: Default::default(),
            st_ctim: Default::default(),
        }
    }
}

/// Workaround implementation because `wit-bindgen` does not generate
/// type aliases, but instead creates the same filetype in each module
/// for `use` statements in the `.wit` file.
impl From<Clockid> for Snapshot0Clockid {
    fn from(other: Clockid) -> Self {
        match other {
            Clockid::Realtime => Self::Realtime,
            Clockid::Monotonic => Self::Monotonic,
            Clockid::ProcessCputimeId => Self::ProcessCputimeId,
            Clockid::ThreadCputimeId => Self::ThreadCputimeId,
            Clockid::Unknown => Self::Unknown,
        }
    }
}

impl From<Snapshot0Clockid> for Clockid {
    fn from(other: Snapshot0Clockid) -> Self {
        match other {
            Snapshot0Clockid::Realtime => Self::Realtime,
            Snapshot0Clockid::Monotonic => Self::Monotonic,
            Snapshot0Clockid::ProcessCputimeId => Self::ProcessCputimeId,
            Snapshot0Clockid::ThreadCputimeId => Self::ThreadCputimeId,
            Snapshot0Clockid::Unknown => Self::Unknown,
        }
    }
}

impl From<Snapshot0SubscriptionClock> for SubscriptionClock {
    fn from(other: Snapshot0SubscriptionClock) -> Self {
        // TODO: this removes Snapshot0SubscriptionClock::identifier, I don't
        // think this is how it should be
        Self {
            clock_id: Clockid::from(other.id),
            timeout: other.timeout,
            precision: other.precision,
            flags: other.flags,
        }
    }
}

impl std::fmt::Display for Sockoption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match *self {
            Self::Noop => "Sockoption::Noop",
            Self::ReusePort => "Sockoption::ReusePort",
            Self::ReuseAddr => "Sockoption::ReuseAddr",
            Self::NoDelay => "Sockoption::NoDelay",
            Self::DontRoute => "Sockoption::DontRoute",
            Self::OnlyV6 => "Sockoption::OnlyV6",
            Self::Broadcast => "Sockoption::Broadcast",
            Self::MulticastLoopV4 => "Sockoption::MulticastLoopV4",
            Self::MulticastLoopV6 => "Sockoption::MulticastLoopV6",
            Self::Promiscuous => "Sockoption::Promiscuous",
            Self::Listening => "Sockoption::Listening",
            Self::LastError => "Sockoption::LastError",
            Self::KeepAlive => "Sockoption::KeepAlive",
            Self::Linger => "Sockoption::Linger",
            Self::OobInline => "Sockoption::OobInline",
            Self::RecvBufSize => "Sockoption::RecvBufSize",
            Self::SendBufSize => "Sockoption::SendBufSize",
            Self::RecvLowat => "Sockoption::RecvLowat",
            Self::SendLowat => "Sockoption::SendLowat",
            Self::RecvTimeout => "Sockoption::RecvTimeout",
            Self::SendTimeout => "Sockoption::SendTimeout",
            Self::ConnectTimeout => "Sockoption::ConnectTimeout",
            Self::AcceptTimeout => "Sockoption::AcceptTimeout",
            Self::Ttl => "Sockoption::Ttl",
            Self::MulticastTtlV4 => "Sockoption::MulticastTtlV4",
            Self::Type => "Sockoption::Type",
            Self::Proto => "Sockoption::Proto",
        };
        write!(f, "{}", s)
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wasmer::FromToNativeWasmType for Fdflags {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self.bits() as i32
    }
    fn from_native(n: Self::Native) -> Self {
        Self::from_bits_truncate(n as u16)
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wasmer::FromToNativeWasmType for Rights {
    type Native = i64;

    fn to_native(self) -> Self::Native {
        self.bits() as i64
    }

    fn from_native(n: Self::Native) -> Self {
        Self::from_bits_truncate(n as u64)
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wasmer::FromToNativeWasmType for Fstflags {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self.bits() as i32
    }
    fn from_native(n: Self::Native) -> Self {
        Self::from_bits_truncate(n as u16)
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wasmer::FromToNativeWasmType for Oflags {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self.bits() as i32
    }
    fn from_native(n: Self::Native) -> Self {
        Self::from_bits_truncate(n as u16)
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

#[derive(Copy, Clone)]
pub enum PrestatEnum {
    Dir { pr_name_len: u32 },
}

impl PrestatEnum {
    pub fn untagged(self) -> PrestatU {
        match self {
            PrestatEnum::Dir { pr_name_len } => PrestatU {
                dir: PrestatUDir { pr_name_len },
            },
        }
    }
}

impl Prestat {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn tagged(&self) -> Option<PrestatEnum> {
        match self.pr_type {
            Preopentype::Dir => Some(PrestatEnum::Dir {
                pr_name_len: self.u.dir.pr_name_len,
            }),
            Preopentype::Unknown => None,
        }
    }
}

unsafe impl wasmer::ValueType for Prestat {
    fn zero_padding_bytes(&self, bytes: &mut [core::mem::MaybeUninit<u8>]) {
        macro_rules! field {
            ($($f:tt)*) => {
                &self.$($f)* as *const _ as usize - self as *const _ as usize
            };
        }
        macro_rules! field_end {
            ($($f:tt)*) => {
                field!($($f)*) + core::mem::size_of_val(&self.$($f)*)
            };
        }
        macro_rules! zero {
            ($start:expr, $end:expr) => {
                for i in $start..$end {
                    bytes[i] = core::mem::MaybeUninit::new(0);
                }
            };
        }
        self.pr_type
            .zero_padding_bytes(&mut bytes[field!(pr_type)..field_end!(pr_type)]);
        zero!(field_end!(pr_type), field!(u));
        match self.pr_type {
            Preopentype::Dir => {
                self.u
                    .dir
                    .zero_padding_bytes(&mut bytes[field!(u.dir)..field_end!(u.dir)]);
                zero!(field_end!(u.dir), field_end!(u));
            }
            Preopentype::Unknown => {}
        }
        zero!(field_end!(u), core::mem::size_of_val(self));
    }
}

impl From<Errno> for std::io::ErrorKind {
    fn from(err: Errno) -> Self {
        use std::io::ErrorKind;
        match err {
            Errno::Access => ErrorKind::PermissionDenied,
            Errno::Addrinuse => ErrorKind::AddrInUse,
            Errno::Addrnotavail => ErrorKind::AddrNotAvailable,
            Errno::Again => ErrorKind::WouldBlock,
            Errno::Already => ErrorKind::AlreadyExists,
            Errno::Badf => ErrorKind::InvalidInput,
            Errno::Badmsg => ErrorKind::InvalidData,
            Errno::Canceled => ErrorKind::Interrupted,
            Errno::Connaborted => ErrorKind::ConnectionAborted,
            Errno::Connrefused => ErrorKind::ConnectionRefused,
            Errno::Connreset => ErrorKind::ConnectionReset,
            Errno::Exist => ErrorKind::AlreadyExists,
            Errno::Intr => ErrorKind::Interrupted,
            Errno::Inval => ErrorKind::InvalidInput,
            Errno::Netreset => ErrorKind::ConnectionReset,
            Errno::Noent => ErrorKind::NotFound,
            Errno::Nomem => ErrorKind::OutOfMemory,
            Errno::Nomsg => ErrorKind::InvalidData,
            Errno::Notconn => ErrorKind::NotConnected,
            Errno::Perm => ErrorKind::PermissionDenied,
            Errno::Pipe => ErrorKind::BrokenPipe,
            Errno::Timedout => ErrorKind::TimedOut,
            _ => ErrorKind::Other,
        }
    }
}

impl From<Errno> for std::io::Error {
    fn from(err: Errno) -> Self {
        let kind: std::io::ErrorKind = err.into();
        std::io::Error::new(kind, err.message())
    }
}

impl From<std::io::Error> for Errno {
    fn from(err: std::io::Error) -> Self {
        use std::io::ErrorKind;
        match err.kind() {
            ErrorKind::NotFound => Errno::Noent,
            ErrorKind::PermissionDenied => Errno::Perm,
            ErrorKind::ConnectionRefused => Errno::Connrefused,
            ErrorKind::ConnectionReset => Errno::Connreset,
            ErrorKind::ConnectionAborted => Errno::Connaborted,
            ErrorKind::NotConnected => Errno::Notconn,
            ErrorKind::AddrInUse => Errno::Addrinuse,
            ErrorKind::AddrNotAvailable => Errno::Addrnotavail,
            ErrorKind::BrokenPipe => Errno::Pipe,
            ErrorKind::AlreadyExists => Errno::Exist,
            ErrorKind::WouldBlock => Errno::Again,
            ErrorKind::InvalidInput => Errno::Io,
            ErrorKind::InvalidData => Errno::Io,
            ErrorKind::TimedOut => Errno::Timedout,
            ErrorKind::WriteZero => Errno::Io,
            ErrorKind::Interrupted => Errno::Intr,
            ErrorKind::Other => Errno::Io,
            ErrorKind::UnexpectedEof => Errno::Io,
            ErrorKind::Unsupported => Errno::Notsup,
            _ => Errno::Io,
        }
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wasmer::FromToNativeWasmType for JoinFlags {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self.bits() as i32
    }
    fn from_native(n: Self::Native) -> Self {
        Self::from_bits_truncate(n as u32)
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}
