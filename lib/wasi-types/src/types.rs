#![deny(unused_mut)]
#![allow(non_camel_case_types, clippy::identity_op)]

//! Wasmer's WASI types implementation.
//!
//! Those types aim at being used by [the `wasmer-wasi`
//! crate](https://github.com/wasmerio/wasmer/blob/master/lib/wasi).

// Needed for #[derive(ValueType)]
extern crate wasmer_types as wasmer;

pub use crate::types::time::*;
pub use directory::*;
pub use file::*;
pub use io::*;
pub use net::*;
pub use signal::*;
pub use subscription::*;

pub mod file {
    use crate::wasi::{Fd, Rights};

    pub use crate::wasi::{EventFdFlags, FileDelta, LookupFlags, Oflags};

    pub const __WASI_STDIN_FILENO: Fd = 0;
    pub const __WASI_STDOUT_FILENO: Fd = 1;
    pub const __WASI_STDERR_FILENO: Fd = 2;

    pub const EVENT_FD_FLAGS_SEMAPHORE: EventFdFlags = 1;

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
    use crate::wasi;
    use std::mem;

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
        use crate::wasi;

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

    pub use crate::wasi::Bool;
    pub use crate::wasi::Count;
    pub use crate::wasi::OptionTag;
    pub use crate::wasi::StdioMode;

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

pub mod time {
    pub use crate::wasi::OptionTimestamp;
}

pub mod net {
    use crate::wasi::Addressfamily;
    use wasmer_derive::ValueType;

    use crate::wasi::OptionTimestamp;

    pub use crate::wasi::{
        AddrUnspec, AddrUnspecPort, CidrUnspec, HttpHandles, HttpStatus, RiFlags, RoFlags, SdFlags,
        SiFlags, SockProto, Timeout,
    };

    #[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
    #[repr(C)]
    pub struct __wasi_hardwareaddress_t {
        pub octs: [u8; 6],
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
        // C will add a padding byte here which must be set to zero otherwise the tag will corrupt
        pub _padding: u8,
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
        // C will add a padding byte here which must be set to zero otherwise the tag will corrupt
        pub _padding: u8,
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
        // C will add a padding byte here which must be set to zero otherwise the tag will corrupt
        pub _padding: u8,
        pub u: __wasi_cidr_u,
    }

    #[derive(Debug, Copy, Clone, ValueType)]
    #[repr(C)]
    pub struct Route {
        pub cidr: __wasi_cidr_t,
        pub via_router: __wasi_addr_t,
        pub preferred_until: OptionTimestamp,
        pub expires_at: OptionTimestamp,
    }

    pub const __WASI_SOCK_RECV_INPUT_PEEK: RiFlags = 1 << 0;
    pub const __WASI_SOCK_RECV_INPUT_WAITALL: RiFlags = 1 << 1;
    pub const __WASI_SOCK_RECV_INPUT_DATA_TRUNCATED: RiFlags = 1 << 2;

    pub const __WASI_SOCK_RECV_OUTPUT_DATA_TRUNCATED: RoFlags = 1 << 0;

    pub const __WASI_SHUT_RD: SdFlags = 1 << 0;
    pub const __WASI_SHUT_WR: SdFlags = 1 << 1;
}

pub mod signal {
    pub use crate::wasi::Signal;
}

pub mod subscription {
    pub use crate::wasi::{Eventtype, SubscriptionFsReadwrite};
}
