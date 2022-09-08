#![deny(unused_mut)]
#![doc(html_favicon_url = "https://wasmer.io/images/icons/favicon-32x32.png")]
#![doc(html_logo_url = "https://github.com/wasmerio.png?size=200")]
#![allow(non_camel_case_types, clippy::identity_op)]

//! Wasmer's WASI types implementation.
//!
//! Those types aim at being used by [the `wasmer-wasi`
//! crate](https://github.com/wasmerio/wasmer/blob/master/lib/wasi).

// Needed for #[derive(ValueType)]
extern crate wasmer_types as wasmer;

pub use crate::time::*;
pub use bus::*;
pub use directory::*;
pub use file::*;
pub use io::*;
pub use net::*;
pub use signal::*;
pub use subscription::*;

pub type __wasi_exitcode_t = u32;
pub type __wasi_userdata_t = u64;

pub mod bus {
    use wasmer_derive::ValueType;
    use wasmer_types::MemorySize;
    use wasmer_wasi_types_generated::wasi::{
        BusDataFormat, BusEventClose, BusEventExit, BusEventFault, BusEventType, Cid, OptionCid,
    };

    // Not sure how to port these types to .wit with generics ...

    #[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
    #[repr(C)]
    pub struct __wasi_busevent_call_t<M: MemorySize> {
        pub parent: OptionCid,
        pub cid: Cid,
        pub format: BusDataFormat,
        pub topic_ptr: M::Offset,
        pub topic_len: M::Offset,
        pub buf_ptr: M::Offset,
        pub buf_len: M::Offset,
    }

    #[derive(Copy, Clone)]
    #[repr(C)]
    pub union __wasi_busevent_u<M: MemorySize> {
        pub noop: u8,
        pub exit: BusEventExit,
        pub call: __wasi_busevent_call_t<M>,
        pub result: __wasi_busevent_result_t<M>,
        pub fault: BusEventFault,
        pub close: BusEventClose,
    }

    #[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
    #[repr(C)]
    pub struct __wasi_busevent_result_t<M: MemorySize> {
        pub format: BusDataFormat,
        pub cid: Cid,
        pub buf_ptr: M::Offset,
        pub buf_len: M::Offset,
    }

    #[derive(Copy, Clone)]
    #[repr(C)]
    pub struct __wasi_busevent_t<M: MemorySize> {
        pub tag: BusEventType,
        pub u: __wasi_busevent_u<M>,
    }
}

pub mod file {
    use std::{
        fmt,
        mem::{self, MaybeUninit},
    };
    use wasmer_derive::ValueType;
    use wasmer_types::ValueType;
    use wasmer_wasi_types_generated::wasi::{Fd, Preopentype, Prestat, Rights};

    pub use wasmer_wasi_types_generated::wasi::{EventFdFlags, FileDelta, LookupFlags, Oflags};

    pub const __WASI_STDIN_FILENO: Fd = 0;
    pub const __WASI_STDOUT_FILENO: Fd = 1;
    pub const __WASI_STDERR_FILENO: Fd = 2;

    pub const EventFdFlags_SEMAPHORE: EventFdFlags = 1;

    pub const __WASI_LOOKUP_SYMLINK_FOLLOW: LookupFlags = 1;

    /// function for debugging rights issues
    #[allow(dead_code)]
    pub fn print_right_set(rights: Rights) {
        // BTreeSet for consistent order
        let mut right_set = std::collections::BTreeSet::new();
        for i in 0..28 {
            let cur_right = rights & Rights::from_bits(1 << i).unwrap();
            if !cur_right.is_empty() {
                right_set.insert(cur_right.to_str().unwrap_or("INVALID RIGHT"));
            }
        }
        println!("{:#?}", right_set);
    }
}

pub mod directory {
    use std::mem;
    use wasmer_wasi_types_generated::wasi;

    pub const __WASI_DIRCOOKIE_START: wasi::Dircookie = 0;

    pub fn dirent_to_le_bytes(ent: &wasi::Dirent) -> Vec<u8> {
        let out: Vec<u8> = std::iter::empty()
            .chain(ent.d_next.to_le_bytes())
            .chain(ent.d_ino.to_le_bytes())
            .chain(ent.d_namlen.to_le_bytes())
            .chain(u32::from(ent.d_type as u8).to_le_bytes())
            .collect();

        assert_eq!(out.len(), mem::size_of::<wasi::Dirent>());
        out
    }

    #[cfg(test)]
    mod tests {
        use super::dirent_to_le_bytes;
        use wasmer_wasi_types_generated::wasi;

        #[test]
        fn test_dirent_to_le_bytes() {
            let s = wasi::Dirent {
                d_next: 0x0123456789abcdef,
                d_ino: 0xfedcba9876543210,
                d_namlen: 0xaabbccdd,
                d_type: wasi::Filetype::Directory,
            };

            assert_eq!(
                vec![
                    // d_next
                    0xef,
                    0xcd,
                    0xab,
                    0x89,
                    0x67,
                    0x45,
                    0x23,
                    0x01,
                    //
                    // d_ino
                    0x10,
                    0x32,
                    0x54,
                    0x76,
                    0x98,
                    0xba,
                    0xdc,
                    0xfe,
                    //
                    // d_namelen
                    0xdd,
                    0xcc,
                    0xbb,
                    0xaa,
                    //
                    // d_type
                    // plus padding
                    wasi::Filetype::Directory as u8,
                    0x00,
                    0x00,
                    0x00,
                ],
                dirent_to_le_bytes(&s)
            );
        }
    }
}

pub mod io {
    use wasmer_derive::ValueType;
    use wasmer_types::MemorySize;
    use wasmer_wasi_types_generated::wasi::Fd;

    pub use wasmer_wasi_types_generated::wasi::Bool;
    pub use wasmer_wasi_types_generated::wasi::Count;
    pub use wasmer_wasi_types_generated::wasi::OptionTag;
    pub use wasmer_wasi_types_generated::wasi::StdioMode;

    #[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
    #[repr(C)]
    pub struct __wasi_ciovec_t<M: MemorySize> {
        pub buf: M::Offset,
        pub buf_len: M::Offset,
    }

    #[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
    #[repr(C)]
    pub struct __wasi_iovec_t<M: MemorySize> {
        pub buf: M::Offset,
        pub buf_len: M::Offset,
    }
}

// --- not ported

pub mod net {
    use super::*;
    use wasmer_derive::ValueType;
    use wasmer_wasi_types_generated::wasi::{Addressfamily, Fd, Filesize};

    use crate::OptionTimestamp;

    pub use wasmer_wasi_types_generated::wasi::SockProto;

    #[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
    #[repr(C)]
    pub struct __wasi_hardwareaddress_t {
        pub octs: [u8; 6],
    }

    #[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
    #[repr(C)]
    pub struct __wasi_addr_unspec_t {
        pub n0: u8,
    }

    #[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
    #[repr(C)]
    pub struct __wasi_addr_unspec_port_t {
        pub port: u16,
        pub addr: __wasi_addr_unspec_t,
    }

    #[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
    #[repr(C)]
    pub struct __wasi_cidr_unspec_t {
        pub addr: __wasi_addr_unspec_t,
        pub prefix: u8,
    }

    #[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
    #[repr(C)]
    pub struct __wasi_addr_ip4_t {
        pub octs: [u8; 4],
    }

    #[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
    #[repr(C)]
    pub struct __wasi_addr_ip4_port_t {
        pub port: u16,
        pub ip: __wasi_addr_ip4_t,
    }

    #[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
    #[repr(C)]
    pub struct __wasi_cidr_ip4_t {
        pub ip: __wasi_addr_ip4_t,
        pub prefix: u8,
    }

    #[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
    #[repr(C)]
    pub struct __wasi_addr_unix_t {
        pub octs: [u8; 16],
    }

    #[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
    #[repr(C)]
    pub struct __wasi_addr_unix_port_t {
        pub port: u16,
        pub unix: __wasi_addr_unix_t,
    }

    #[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
    #[repr(C)]
    pub struct __wasi_cidr_unix_t {
        pub unix: __wasi_addr_unix_t,
        pub prefix: u8,
    }

    #[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
    #[repr(C)]
    pub struct __wasi_addr_ip6_t {
        pub segs: [u8; 16],
    }

    #[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
    #[repr(C)]
    pub struct __wasi_addr_ip6_port_t {
        pub port: u16,
        pub ip: __wasi_addr_ip6_t,
    }

    #[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
    #[repr(C)]
    pub struct __wasi_cidr_ip6_t {
        pub ip: __wasi_addr_ip6_t,
        pub prefix: u8,
    }

    #[derive(Debug, Copy, Clone, ValueType)]
    #[repr(C)]
    pub struct __wasi_addr_u {
        pub octs: [u8; 16],
    }

    #[derive(Debug, Copy, Clone, ValueType)]
    #[repr(C)]
    pub struct __wasi_addr_t {
        pub tag: Addressfamily,
        pub u: __wasi_addr_u,
    }

    #[derive(Debug, Copy, Clone, ValueType)]
    #[repr(C)]
    pub struct __wasi_addr_port_u {
        pub octs: [u8; 18],
    }

    #[derive(Debug, Copy, Clone, ValueType)]
    #[repr(C)]
    pub struct __wasi_addr_port_t {
        pub tag: Addressfamily,
        pub u: __wasi_addr_port_u,
    }

    #[derive(Debug, Copy, Clone, ValueType)]
    #[repr(C)]
    pub struct __wasi_cidr_u {
        pub octs: [u8; 17],
    }

    #[derive(Debug, Copy, Clone, ValueType)]
    #[repr(C)]
    pub struct __wasi_cidr_t {
        pub tag: Addressfamily,
        pub u: __wasi_cidr_u,
    }

    #[derive(Debug, Copy, Clone, ValueType)]
    #[repr(C)]
    pub struct __wasi_route_t {
        pub cidr: __wasi_cidr_t,
        pub via_router: __wasi_addr_t,
        pub preferred_until: OptionTimestamp,
        pub expires_at: OptionTimestamp,
    }

    #[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
    #[repr(C)]
    pub struct __wasi_http_handles_t {
        pub req: Fd,
        pub res: Fd,
        pub hdr: Fd,
    }

    #[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
    #[repr(C)]
    pub struct __wasi_http_status_t {
        pub ok: Bool,
        pub redirect: Bool,
        pub size: Filesize,
        pub status: u16,
    }

    pub type __wasi_riflags_t = u16;
    pub const __WASI_SOCK_RECV_INPUT_PEEK: __wasi_riflags_t = 1 << 0;
    pub const __WASI_SOCK_RECV_INPUT_WAITALL: __wasi_riflags_t = 1 << 1;
    pub const __WASI_SOCK_RECV_INPUT_DATA_TRUNCATED: __wasi_riflags_t = 1 << 2;

    pub type __wasi_roflags_t = u16;
    pub const __WASI_SOCK_RECV_OUTPUT_DATA_TRUNCATED: __wasi_roflags_t = 1 << 0;

    pub type __wasi_sdflags_t = u8;
    pub const __WASI_SHUT_RD: __wasi_sdflags_t = 1 << 0;
    pub const __WASI_SHUT_WR: __wasi_sdflags_t = 1 << 1;

    pub type __wasi_siflags_t = u16;

    pub type __wasi_timeout_t = u8;
    pub const __WASI_TIMEOUT_READ: __wasi_timeout_t = 0;
    pub const __WASI_TIMEOUT_WRITE: __wasi_timeout_t = 1;
    pub const __WASI_TIMEOUT_CONNECT: __wasi_timeout_t = 2;
    pub const __WASI_TIMEOUT_ACCEPT: __wasi_timeout_t = 3;
}

pub mod signal {
    pub type __wasi_signal_t = u8;
    pub const __WASI_SIGHUP: u8 = 1;
    pub const __WASI_SIGINT: u8 = 2;
    pub const __WASI_SIGQUIT: u8 = 3;
    pub const __WASI_SIGILL: u8 = 4;
    pub const __WASI_SIGTRAP: u8 = 5;
    pub const __WASI_SIGABRT: u8 = 6;
    pub const __WASI_SIGBUS: u8 = 7;
    pub const __WASI_SIGFPE: u8 = 8;
    pub const __WASI_SIGKILL: u8 = 9;
    pub const __WASI_SIGUSR1: u8 = 10;
    pub const __WASI_SIGSEGV: u8 = 11;
    pub const __WASI_SIGUSR2: u8 = 12;
    pub const __WASI_SIGPIPE: u8 = 13;
    pub const __WASI_SIGALRM: u8 = 14;
    pub const __WASI_SIGTERM: u8 = 15;
    pub const __WASI_SIGCHLD: u8 = 16;
    pub const __WASI_SIGCONT: u8 = 17;
    pub const __WASI_SIGSTOP: u8 = 18;
    pub const __WASI_SIGTSTP: u8 = 19;
    pub const __WASI_SIGTTIN: u8 = 20;
    pub const __WASI_SIGTTOU: u8 = 21;
    pub const __WASI_SIGURG: u8 = 22;
    pub const __WASI_SIGXCPU: u8 = 23;
    pub const __WASI_SIGXFSZ: u8 = 24;
    pub const __WASI_SIGVTALRM: u8 = 25;
    pub const __WASI_SIGPROF: u8 = 26;
    pub const __WASI_SIGWINCH: u8 = 27;
    pub const __WASI_SIGPOLL: u8 = 28;
    pub const __WASI_SIGPWR: u8 = 29;
    pub const __WASI_SIGSYS: u8 = 30;
}

pub mod subscription {
    use wasmer_wasi_types_generated::wasi::{
        Eventtype, SubscriptionClock, SubscriptionFsReadwrite,
    };

    /// Safe Rust wrapper around `__wasi_subscription_t::type_` and `__wasi_subscription_t::u`
    #[derive(Debug, Clone)]
    pub enum EventType {
        Clock(SubscriptionClock),
        Read(SubscriptionFsReadwrite),
        Write(SubscriptionFsReadwrite),
    }

    impl EventType {
        pub fn raw_tag(&self) -> Eventtype {
            match self {
                EventType::Clock(_) => Eventtype::Clock,
                EventType::Read(_) => Eventtype::FdRead,
                EventType::Write(_) => Eventtype::FdWrite,
            }
        }
    }

    /* TODO: re-enable and adjust if still required
    impl TryFrom<WasiSubscription> for __wasi_subscription_t {
        type Error = Errno;

        fn try_from(ws: WasiSubscription) -> Result<Self, Self::Error> {
            #[allow(unreachable_patterns)]
            let (type_, u) = match ws.event_type {
                EventType::Clock(c) => (Eventtype::Clock, __wasi_subscription_u { clock: c }),
                EventType::Read(rw) => (
                    Eventtype::FdRead,
                    __wasi_subscription_u { fd_readwrite: rw },
                ),
                EventType::Write(rw) => (
                    Eventtype::FdWrite,
                    __wasi_subscription_u { fd_readwrite: rw },
                ),
                _ => return Err(Errno::Inval),
            };

            Ok(Self {
                userdata: ws.user_data,
                type_,
                u,
            })
        }
    }

    impl fmt::Debug for __wasi_subscription_t {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("__wasi_subscription_t")
                .field("userdata", &self.userdata)
                .field("type", &self.type_.to_str())
                .field(
                    "u",
                    match self.type_ {
                        Eventtype::Clock => unsafe { &self.u.clock },
                        Eventtype::FdRead | Eventtype::FdWrite => unsafe { &self.u.fd_readwrite },
                    },
                )
                .finish()
        }
    }

    unsafe impl ValueType for __wasi_subscription_t {
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
            self.userdata
                .zero_padding_bytes(&mut bytes[field!(userdata)..field_end!(userdata)]);
            zero!(field_end!(userdata), field!(type_));
            self.type_
                .zero_padding_bytes(&mut bytes[field!(type_)..field_end!(type_)]);
            zero!(field_end!(type_), field!(u));
            match self.type_ {
                Eventtype::FdRead | Eventtype::FdWrite => unsafe {
                    self.u.fd_readwrite.zero_padding_bytes(
                        &mut bytes[field!(u.fd_readwrite)..field_end!(u.fd_readwrite)],
                    );
                    zero!(field_end!(u.fd_readwrite), field_end!(u));
                },
                Eventtype::Clock => unsafe {
                    self.u
                        .clock
                        .zero_padding_bytes(&mut bytes[field!(u.clock)..field_end!(u.clock)]);
                    zero!(field_end!(u.clock), field_end!(u));
                },
            }
            zero!(field_end!(u), mem::size_of_val(self));
        }
    }

    pub enum SubscriptionEnum {
        Clock(__wasi_subscription_clock_t),
        FdReadWrite(__wasi_subscription_fs_readwrite_t),
    }

    impl __wasi_subscription_t {
        pub fn tagged(&self) -> Option<SubscriptionEnum> {
            match self.type_ {
                Eventtype::Clock => Some(SubscriptionEnum::Clock(unsafe { self.u.clock })),
                Eventtype::FdRead | Eventtype::FdWrite => Some(SubscriptionEnum::FdReadWrite(unsafe {
                    self.u.fd_readwrite
                })),
            }
        }
    }

    */
}

pub mod time {
    use wasmer_derive::ValueType;
    pub use wasmer_wasi_types_generated::wasi::OptionTimestamp;
    use wasmer_wasi_types_generated::wasi::{OptionTag, Timestamp};
}
