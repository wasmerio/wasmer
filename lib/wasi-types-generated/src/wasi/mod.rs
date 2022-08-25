mod bindings;
pub use bindings::wasi::*;

use std::mem::MaybeUninit;
use wasmer_types::ValueType;

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Errno {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for BusErrno {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Filetype {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Socktype {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Sockstatus {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Sockoption {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Streamsecurity {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Addressfamily {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Snapshot0Whence {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

impl Filetype {
    pub fn name(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::BlockDevice => "Block device",
            Self::CharacterDevice => "Character device",
            Self::Directory => "Directory",
            Self::RegularFile => "Regular file",
            Self::SocketDgram => "Socket dgram",
            Self::SocketStream => "Socket stream",
            Self::SymbolicLink => "Symbolic link",
            Self::Fifo => "Fifo",
        }
    }
}

unsafe impl ValueType for Event {
    fn zero_padding_bytes(&self, bytes: &mut [MaybeUninit<u8>]) {
        macro_rules! field {
            ($($f:tt)*) => {
                &self.$($f)* as *const _ as usize - self as *const _ as usize
            };
        }
        macro_rules! field_end {
            ($($f:tt)*) => {
                field!($($f)*) + std::mem::size_of_val(&self.$($f)*)
            };
        }
        macro_rules! zero {
            ($start:expr, $end:expr) => {
                for i in $start..$end {
                    bytes[i] = MaybeUninit::new(0);
                }
            };
        }
        self.userdata
            .zero_padding_bytes(&mut bytes[field!(userdata)..field_end!(userdata)]);
        zero!(field_end!(userdata), field!(error));
        self.error
            .zero_padding_bytes(&mut bytes[field!(error)..field_end!(error)]);
        zero!(field_end!(error), field!(data));
        self.data
            .zero_padding_bytes(&mut bytes[field!(data)..field_end!(data)]);
        zero!(field_end!(data), std::mem::size_of_val(self));
    }
}

unsafe impl ValueType for Tty {
    fn zero_padding_bytes(&self, bytes: &mut [MaybeUninit<u8>]) {
        macro_rules! field {
            ($($f:tt)*) => {
                &self.$($f)* as *const _ as usize - self as *const _ as usize
            };
        }
        macro_rules! field_end {
            ($($f:tt)*) => {
                field!($($f)*) + std::mem::size_of_val(&self.$($f)*)
            };
        }
        macro_rules! zero {
            ($start:expr, $end:expr) => {
                for i in $start..$end {
                    bytes[i] = MaybeUninit::new(0);
                }
            };
        }

        self.cols
            .zero_padding_bytes(&mut bytes[field!(cols)..field_end!(cols)]);
        zero!(field_end!(cols), field!(rows));

        self.rows
            .zero_padding_bytes(&mut bytes[field!(rows)..field_end!(rows)]);
        zero!(field_end!(rows), field!(width));

        self.width
            .zero_padding_bytes(&mut bytes[field!(width)..field_end!(width)]);
        zero!(field_end!(width), field!(height));

        self.height
            .zero_padding_bytes(&mut bytes[field!(height)..field_end!(height)]);
        zero!(field_end!(height), field!(stdin_tty));

        self.stdin_tty
            .zero_padding_bytes(&mut bytes[field!(stdin_tty)..field_end!(stdin_tty)]);
        zero!(field_end!(stdin_tty), field!(stdout_tty));

        self.stdout_tty
            .zero_padding_bytes(&mut bytes[field!(stdout_tty)..field_end!(stdout_tty)]);
        zero!(field_end!(stdout_tty), field!(stderr_tty));

        self.stderr_tty
            .zero_padding_bytes(&mut bytes[field!(stderr_tty)..field_end!(stderr_tty)]);
        zero!(field_end!(stderr_tty), field!(echo));

        self.echo
            .zero_padding_bytes(&mut bytes[field!(echo)..field_end!(echo)]);
        zero!(field_end!(echo), field!(line_buffered));

        self.line_buffered
            .zero_padding_bytes(&mut bytes[field!(line_buffered)..field_end!(line_buffered)]);
        zero!(field_end!(line_buffered), std::mem::size_of_val(self));
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Eventtype {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Rights {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

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

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Eventrwflags {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Fstflags {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for SubscriptionEnum {
    fn zero_padding_bytes(&self, bytes: &mut [MaybeUninit<u8>]) {
        macro_rules! field {
            ($($f:tt)*) => {
                &$($f)* as *const _ as usize - self as *const _ as usize
            };
        }
        macro_rules! field_end {
            ($($f:tt)*) => {
                field!($($f)*) + std::mem::size_of_val(&$($f)*)
            };
        }
        macro_rules! zero {
            ($start:expr, $end:expr) => {
                for i in $start..$end {
                    bytes[i] = MaybeUninit::new(0);
                }
            };
        }

        match self {
            SubscriptionEnum::Clock(d) => {
                d.zero_padding_bytes(&mut bytes[field!(d)..field_end!(d)]);
                zero!(field_end!(d), std::mem::size_of_val(self))
            }
            SubscriptionEnum::Read(d) => {
                d.zero_padding_bytes(&mut bytes[field!(d)..field_end!(d)]);
                zero!(field_end!(d), std::mem::size_of_val(self))
            }
            SubscriptionEnum::Write(d) => {
                d.zero_padding_bytes(&mut bytes[field!(d)..field_end!(d)]);
                zero!(field_end!(d), std::mem::size_of_val(self))
            }
        }
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for EventEnum {
    fn zero_padding_bytes(&self, bytes: &mut [MaybeUninit<u8>]) {
        macro_rules! field {
            ($($f:tt)*) => {
                &$($f)* as *const _ as usize - self as *const _ as usize
            };
        }
        macro_rules! field_end {
            ($($f:tt)*) => {
                field!($($f)*) + std::mem::size_of_val(&$($f)*)
            };
        }
        macro_rules! zero {
            ($start:expr, $end:expr) => {
                for i in $start..$end {
                    bytes[i] = MaybeUninit::new(0);
                }
            };
        }

        match self {
            Self::Clock => {
                zero!(field!(self), std::mem::size_of_val(self))
            }
            Self::FdRead(d) => {
                d.zero_padding_bytes(&mut bytes[field!(d)..field_end!(d)]);
                zero!(field_end!(d), std::mem::size_of_val(self))
            }
            Self::FdWrite(d) => {
                d.zero_padding_bytes(&mut bytes[field!(d)..field_end!(d)]);
                zero!(field_end!(d), std::mem::size_of_val(self))
            }
        }
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Snapshot0SubscriptionEnum {
    fn zero_padding_bytes(&self, bytes: &mut [MaybeUninit<u8>]) {
        macro_rules! field {
            ($($f:tt)*) => {
                &$($f)* as *const _ as usize - self as *const _ as usize
            };
        }
        macro_rules! field_end {
            ($($f:tt)*) => {
                field!($($f)*) + std::mem::size_of_val(&$($f)*)
            };
        }
        macro_rules! zero {
            ($start:expr, $end:expr) => {
                for i in $start..$end {
                    bytes[i] = MaybeUninit::new(0);
                }
            };
        }

        match self {
            Snapshot0SubscriptionEnum::Clock(d) => {
                d.zero_padding_bytes(&mut bytes[field!(d)..field_end!(d)]);
                zero!(field_end!(d), std::mem::size_of_val(self))
            }
            Snapshot0SubscriptionEnum::Read(d) => {
                d.zero_padding_bytes(&mut bytes[field!(d)..field_end!(d)]);
                zero!(field_end!(d), std::mem::size_of_val(self))
            }
            Snapshot0SubscriptionEnum::Write(d) => {
                d.zero_padding_bytes(&mut bytes[field!(d)..field_end!(d)]);
                zero!(field_end!(d), std::mem::size_of_val(self))
            }
        }
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Subclockflags {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Clockid {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Fdflags {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Preopentype {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for EventFdReadwrite {
    fn zero_padding_bytes(&self, bytes: &mut [MaybeUninit<u8>]) {
        macro_rules! field {
            ($($f:tt)*) => {
                &self.$($f)* as *const _ as usize - self as *const _ as usize
            };
        }
        macro_rules! field_end {
            ($($f:tt)*) => {
                field!($($f)*) + std::mem::size_of_val(&self.$($f)*)
            };
        }
        macro_rules! zero {
            ($start:expr, $end:expr) => {
                for i in $start..$end {
                    bytes[i] = MaybeUninit::new(0);
                }
            };
        }

        self.nbytes
            .zero_padding_bytes(&mut bytes[field!(nbytes)..field_end!(nbytes)]);
        zero!(field_end!(nbytes), field!(flags));

        self.flags
            .zero_padding_bytes(&mut bytes[field!(flags)..field_end!(flags)]);
        zero!(field_end!(flags), std::mem::size_of_val(self));
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Fdstat {
    fn zero_padding_bytes(&self, bytes: &mut [MaybeUninit<u8>]) {
        macro_rules! field {
            ($($f:tt)*) => {
                &self.$($f)* as *const _ as usize - self as *const _ as usize
            };
        }
        macro_rules! field_end {
            ($($f:tt)*) => {
                field!($($f)*) + std::mem::size_of_val(&self.$($f)*)
            };
        }
        macro_rules! zero {
            ($start:expr, $end:expr) => {
                for i in $start..$end {
                    bytes[i] = MaybeUninit::new(0);
                }
            };
        }

        self.fs_filetype
            .zero_padding_bytes(&mut bytes[field!(fs_filetype)..field_end!(fs_filetype)]);
        zero!(field_end!(fs_filetype), field!(fs_flags));

        self.fs_flags
            .zero_padding_bytes(&mut bytes[field!(fs_flags)..field_end!(fs_flags)]);
        zero!(field_end!(fs_flags), field!(fs_rights_base));

        self.fs_rights_base
            .zero_padding_bytes(&mut bytes[field!(fs_rights_base)..field_end!(fs_rights_base)]);
        zero!(
            field_end!(fs_rights_inheriting),
            std::mem::size_of_val(self)
        );
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Snapshot0SubscriptionClock {
    fn zero_padding_bytes(&self, bytes: &mut [MaybeUninit<u8>]) {
        macro_rules! field {
            ($($f:tt)*) => {
                &self.$($f)* as *const _ as usize - self as *const _ as usize
            };
        }
        macro_rules! field_end {
            ($($f:tt)*) => {
                field!($($f)*) + std::mem::size_of_val(&self.$($f)*)
            };
        }
        macro_rules! zero {
            ($start:expr, $end:expr) => {
                for i in $start..$end {
                    bytes[i] = MaybeUninit::new(0);
                }
            };
        }

        self.identifier
            .zero_padding_bytes(&mut bytes[field!(identifier)..field_end!(identifier)]);
        zero!(field_end!(identifier), field!(id));

        self.id
            .zero_padding_bytes(&mut bytes[field!(id)..field_end!(id)]);
        zero!(field_end!(id), field!(timeout));

        self.timeout
            .zero_padding_bytes(&mut bytes[field!(timeout)..field_end!(timeout)]);
        zero!(field_end!(timeout), field!(precision));

        self.precision
            .zero_padding_bytes(&mut bytes[field!(precision)..field_end!(precision)]);
        zero!(field_end!(precision), field!(flags));

        self.flags
            .zero_padding_bytes(&mut bytes[field!(flags)..field_end!(flags)]);
        zero!(field_end!(flags), std::mem::size_of_val(self));
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for SubscriptionFsReadwrite {
    fn zero_padding_bytes(&self, bytes: &mut [MaybeUninit<u8>]) {
        macro_rules! field {
            ($($f:tt)*) => {
                &self.$($f)* as *const _ as usize - self as *const _ as usize
            };
        }
        macro_rules! field_end {
            ($($f:tt)*) => {
                field!($($f)*) + std::mem::size_of_val(&self.$($f)*)
            };
        }
        macro_rules! zero {
            ($start:expr, $end:expr) => {
                for i in $start..$end {
                    bytes[i] = MaybeUninit::new(0);
                }
            };
        }

        self.file_descriptor
            .zero_padding_bytes(&mut bytes[field!(file_descriptor)..field_end!(file_descriptor)]);
        zero!(field_end!(file_descriptor), std::mem::size_of_val(self));
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for SubscriptionClock {
    fn zero_padding_bytes(&self, bytes: &mut [MaybeUninit<u8>]) {
        macro_rules! field {
            ($($f:tt)*) => {
                &self.$($f)* as *const _ as usize - self as *const _ as usize
            };
        }
        macro_rules! field_end {
            ($($f:tt)*) => {
                field!($($f)*) + std::mem::size_of_val(&self.$($f)*)
            };
        }
        macro_rules! zero {
            ($start:expr, $end:expr) => {
                for i in $start..$end {
                    bytes[i] = MaybeUninit::new(0);
                }
            };
        }

        self.clock_id
            .zero_padding_bytes(&mut bytes[field!(clock_id)..field_end!(clock_id)]);
        zero!(field_end!(clock_id), field!(timeout));

        self.timeout
            .zero_padding_bytes(&mut bytes[field!(timeout)..field_end!(timeout)]);
        zero!(field_end!(timeout), field!(precision));

        self.precision
            .zero_padding_bytes(&mut bytes[field!(precision)..field_end!(precision)]);
        zero!(field_end!(precision), field!(flags));

        self.flags
            .zero_padding_bytes(&mut bytes[field!(flags)..field_end!(flags)]);
        zero!(field_end!(flags), std::mem::size_of_val(self));
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Subscription {
    fn zero_padding_bytes(&self, bytes: &mut [MaybeUninit<u8>]) {
        macro_rules! field {
            ($($f:tt)*) => {
                &self.$($f)* as *const _ as usize - self as *const _ as usize
            };
        }
        macro_rules! field_end {
            ($($f:tt)*) => {
                field!($($f)*) + std::mem::size_of_val(&self.$($f)*)
            };
        }
        macro_rules! zero {
            ($start:expr, $end:expr) => {
                for i in $start..$end {
                    bytes[i] = MaybeUninit::new(0);
                }
            };
        }

        self.userdata
            .zero_padding_bytes(&mut bytes[field!(userdata)..field_end!(userdata)]);
        zero!(field_end!(userdata), field!(data));

        self.data
            .zero_padding_bytes(&mut bytes[field!(data)..field_end!(data)]);
        zero!(field_end!(data), std::mem::size_of_val(self));
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Snapshot0Subscription {
    fn zero_padding_bytes(&self, bytes: &mut [MaybeUninit<u8>]) {
        macro_rules! field {
            ($($f:tt)*) => {
                &self.$($f)* as *const _ as usize - self as *const _ as usize
            };
        }
        macro_rules! field_end {
            ($($f:tt)*) => {
                field!($($f)*) + std::mem::size_of_val(&self.$($f)*)
            };
        }
        macro_rules! zero {
            ($start:expr, $end:expr) => {
                for i in $start..$end {
                    bytes[i] = MaybeUninit::new(0);
                }
            };
        }

        self.userdata
            .zero_padding_bytes(&mut bytes[field!(userdata)..field_end!(userdata)]);
        zero!(field_end!(userdata), field!(data));

        self.data
            .zero_padding_bytes(&mut bytes[field!(data)..field_end!(data)]);
        zero!(field_end!(data), std::mem::size_of_val(self));
    }
}

unsafe impl ValueType for Snapshot0Filestat {
    fn zero_padding_bytes(&self, bytes: &mut [MaybeUninit<u8>]) {
        macro_rules! field {
            ($($f:tt)*) => {
                &self.$($f)* as *const _ as usize - self as *const _ as usize
            };
        }
        macro_rules! field_end {
            ($($f:tt)*) => {
                field!($($f)*) + std::mem::size_of_val(&self.$($f)*)
            };
        }
        macro_rules! zero {
            ($start:expr, $end:expr) => {
                for i in $start..$end {
                    bytes[i] = MaybeUninit::new(0);
                }
            };
        }
        self.st_dev
            .zero_padding_bytes(&mut bytes[field!(st_dev)..field_end!(st_dev)]);
        zero!(field_end!(st_dev), field!(st_ino));

        self.st_ino
            .zero_padding_bytes(&mut bytes[field!(st_ino)..field_end!(st_ino)]);
        zero!(field_end!(st_ino), field!(st_filetype));

        self.st_filetype
            .zero_padding_bytes(&mut bytes[field!(st_filetype)..field_end!(st_filetype)]);
        zero!(field_end!(st_filetype), field!(st_nlink));

        self.st_nlink
            .zero_padding_bytes(&mut bytes[field!(st_nlink)..field_end!(st_nlink)]);
        zero!(field_end!(st_nlink), field!(st_size));

        self.st_size
            .zero_padding_bytes(&mut bytes[field!(st_size)..field_end!(st_size)]);
        zero!(field_end!(st_size), field!(st_atim));

        self.st_atim
            .zero_padding_bytes(&mut bytes[field!(st_atim)..field_end!(st_atim)]);
        zero!(field_end!(st_atim), field!(st_mtim));

        self.st_mtim
            .zero_padding_bytes(&mut bytes[field!(st_mtim)..field_end!(st_mtim)]);
        zero!(field_end!(st_mtim), field!(st_ctim));

        self.st_ctim
            .zero_padding_bytes(&mut bytes[field!(st_ctim)..field_end!(st_ctim)]);
        zero!(field_end!(st_ctim), std::mem::size_of_val(self));
    }
}

unsafe impl ValueType for Filestat {
    fn zero_padding_bytes(&self, bytes: &mut [MaybeUninit<u8>]) {
        macro_rules! field {
            ($($f:tt)*) => {
                &self.$($f)* as *const _ as usize - self as *const _ as usize
            };
        }
        macro_rules! field_end {
            ($($f:tt)*) => {
                field!($($f)*) + std::mem::size_of_val(&self.$($f)*)
            };
        }
        macro_rules! zero {
            ($start:expr, $end:expr) => {
                for i in $start..$end {
                    bytes[i] = MaybeUninit::new(0);
                }
            };
        }
        self.st_dev
            .zero_padding_bytes(&mut bytes[field!(st_dev)..field_end!(st_dev)]);
        zero!(field_end!(st_dev), field!(st_ino));

        self.st_ino
            .zero_padding_bytes(&mut bytes[field!(st_ino)..field_end!(st_ino)]);
        zero!(field_end!(st_ino), field!(st_filetype));

        self.st_filetype
            .zero_padding_bytes(&mut bytes[field!(st_filetype)..field_end!(st_filetype)]);
        zero!(field_end!(st_filetype), field!(st_nlink));

        self.st_nlink
            .zero_padding_bytes(&mut bytes[field!(st_nlink)..field_end!(st_nlink)]);
        zero!(field_end!(st_nlink), field!(st_size));

        self.st_size
            .zero_padding_bytes(&mut bytes[field!(st_size)..field_end!(st_size)]);
        zero!(field_end!(st_size), field!(st_atim));

        self.st_atim
            .zero_padding_bytes(&mut bytes[field!(st_atim)..field_end!(st_atim)]);
        zero!(field_end!(st_atim), field!(st_mtim));

        self.st_mtim
            .zero_padding_bytes(&mut bytes[field!(st_mtim)..field_end!(st_mtim)]);
        zero!(field_end!(st_mtim), field!(st_ctim));

        self.st_ctim
            .zero_padding_bytes(&mut bytes[field!(st_ctim)..field_end!(st_ctim)]);
        zero!(field_end!(st_ctim), std::mem::size_of_val(self));
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

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Errno {
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
            // TODO: What should we map invalid native values to?
            _ => Self::Inval,
        }
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for BusErrno {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }
    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Success,
            1 => Self::Ser,
            2 => Self::Des,
            3 => Self::Wapm,
            4 => Self::Fetch,
            5 => Self::Compile,
            6 => Self::Abi,
            7 => Self::Aborted,
            8 => Self::Badhandle,
            9 => Self::Topic,
            10 => Self::Badcb,
            11 => Self::Unsupported,
            12 => Self::Badrequest,
            13 => Self::Denied,
            14 => Self::Internal,
            15 => Self::Alloc,
            16 => Self::Invoke,
            17 => Self::Consumed,
            18 => Self::Memviolation,
            19 => Self::Unknown,
            // TODO: What should we map invalid native values to?
            _ => Self::Unknown,
        }
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Advice {
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
            // TODO: What should we map invalid native values to?
            _ => todo!("Need to decide what to do here…"),
        }
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Socktype {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }
    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Dgram,
            1 => Self::Stream,
            2 => Self::Raw,
            3 => Self::Seqpacket,
            // TODO: What should we map invalid native values to?
            _ => todo!("Need to decide what to do here…"),
        }
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Sockstatus {
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
            // TODO: What should we map invalid native values to?
            _ => todo!("Need to decide what to do here…"),
        }
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Streamsecurity {
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
            // TODO: What should we map invalid native values to?
            _ => todo!("Need to decide what to do here…"),
        }
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Rights {
    type Native = i64;

    fn to_native(self) -> Self::Native {
        self.bits() as i64
    }

    fn from_native(n: Self::Native) -> Self {
        Self::from_bits_truncate(n as u64)
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Eventrwflags {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self.bits() as i32
    }
    fn from_native(n: Self::Native) -> Self {
        Self::from_bits_truncate(n as u8)
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Fstflags {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self.bits() as i32
    }
    fn from_native(n: Self::Native) -> Self {
        Self::from_bits_truncate(n as u8)
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Subclockflags {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self.bits() as i32
    }
    fn from_native(n: Self::Native) -> Self {
        Self::from_bits_truncate(n as u8)
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Clockid {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }
    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Realtime,
            1 => Self::Monotonic,
            // TODO: What should we map invalid native values to?
            _ => todo!("Need to decide what to do here…"),
        }
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Snapshot0Clockid {
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
            // TODO: What should we map invalid native values to?
            _ => todo!("Need to decide what to do here…"),
        }
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Sockoption {
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
            // TODO: What should we map invalid native values to?
            _ => todo!("Need to decide what to do here…"),
        }
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
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
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Fdflags {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self.bits() as i32
    }
    fn from_native(n: Self::Native) -> Self {
        Self::from_bits_truncate(n as u8)
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Preopentype {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }
    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Dir,
            // TODO: What should we map invalid native values to?
            _ => todo!("Need to decide what to do here…"),
        }
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Addressfamily {
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
            // TODO: What should we map invalid native values to?
            _ => todo!("Need to decide what to do here…"),
        }
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Snapshot0Whence {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }
    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Cur,
            1 => Self::End,
            2 => Self::Set,
            // TODO: What should we map invalid native values to?
            _ => todo!("Need to decide what to do here…"),
        }
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Whence {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }
    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Set,
            1 => Self::Cur,
            2 => Self::End,
            // TODO: What should we map invalid native values to?
            _ => todo!("Need to decide what to do here…"),
        }
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Snapshot0Clockid {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

impl Eventtype {
    pub fn to_str(self) -> &'static str {
        match self {
            Eventtype::Clock => "Wasi::Eventtype::Clock",
            Eventtype::FdRead => "Wasi::Eventtype::FdRead",
            Eventtype::FdWrite => "Wasi::Eventtype::FdWrite",
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
        }
    }
}

impl From<Snapshot0Clockid> for Clockid {
    fn from(other: Snapshot0Clockid) -> Self {
        match other {
            Snapshot0Clockid::Realtime => Self::Realtime,
            Snapshot0Clockid::Monotonic => Self::Monotonic,
            Snapshot0Clockid::ProcessCputimeId => todo!("not implemented for now"),
            Snapshot0Clockid::ThreadCputimeId => todo!("not implemented for now"),
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

impl From<Snapshot0SubscriptionEnum> for SubscriptionEnum {
    fn from(other: Snapshot0SubscriptionEnum) -> Self {
        match other {
            Snapshot0SubscriptionEnum::Clock(d) => Self::Clock(SubscriptionClock::from(d)),
            Snapshot0SubscriptionEnum::Read(d) => Self::Read(d),
            Snapshot0SubscriptionEnum::Write(d) => Self::Write(d),
        }
    }
}

impl From<Snapshot0Subscription> for Subscription {
    fn from(other: Snapshot0Subscription) -> Self {
        Self {
            userdata: other.userdata,
            data: SubscriptionEnum::from(other.data),
        }
    }
}
