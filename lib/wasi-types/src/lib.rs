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

// --- not ported

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

    pub type __wasi_count_t = u32;

    pub type __wasi_option_t = u8;
    pub const __WASI_OPTION_NONE: __wasi_option_t = 0;
    pub const __WASI_OPTION_SOME: __wasi_option_t = 1;

    pub type __wasi_bool_t = u8;
    pub const __WASI_BOOL_FALSE: __wasi_bool_t = 0;
    pub const __WASI_BOOL_TRUE: __wasi_bool_t = 1;

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

    #[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
    #[repr(C)]
    pub struct __wasi_pipe_handles_t {
        pub pipe: Fd,
        pub other: Fd,
    }

    pub type __wasi_stdiomode_t = u8;
    pub const __WASI_STDIO_MODE_PIPED: __wasi_stdiomode_t = 1;
    pub const __WASI_STDIO_MODE_INHERIT: __wasi_stdiomode_t = 2;
    pub const __WASI_STDIO_MODE_NULL: __wasi_stdiomode_t = 3;
    pub const __WASI_STDIO_MODE_LOG: __wasi_stdiomode_t = 4;
}

pub mod net {
    use super::*;
    use wasmer_derive::ValueType;
    use wasmer_wasi_types_generated::wasi::{Addressfamily, Fd, Filesize};

    use crate::__wasi_option_timestamp_t;

    pub type __wasi_sockproto_t = u16;
    pub const __WASI_SOCK_PROTO_IP: __wasi_sockproto_t = 0;
    pub const __WASI_SOCK_PROTO_ICMP: __wasi_sockproto_t = 1;
    pub const __WASI_SOCK_PROTO_IGMP: __wasi_sockproto_t = 2;
    pub const __WASI_SOCK_PROTO_PROTO_3: __wasi_sockproto_t = 3;
    pub const __WASI_SOCK_PROTO_IPIP: __wasi_sockproto_t = 4;
    pub const __WASI_SOCK_PROTO_PROTO_5: __wasi_sockproto_t = 5;
    pub const __WASI_SOCK_PROTO_TCP: __wasi_sockproto_t = 6;
    pub const __WASI_SOCK_PROTO_PROTO_7: __wasi_sockproto_t = 7;
    pub const __WASI_SOCK_PROTO_EGP: __wasi_sockproto_t = 8;
    pub const __WASI_SOCK_PROTO_PROTO_9: __wasi_sockproto_t = 9;
    pub const __WASI_SOCK_PROTO_PROTO_10: __wasi_sockproto_t = 10;
    pub const __WASI_SOCK_PROTO_PROTO_11: __wasi_sockproto_t = 11;
    pub const __WASI_SOCK_PROTO_PUP: __wasi_sockproto_t = 12;
    pub const __WASI_SOCK_PROTO_PROTO_13: __wasi_sockproto_t = 13;
    pub const __WASI_SOCK_PROTO_PROTO_14: __wasi_sockproto_t = 14;
    pub const __WASI_SOCK_PROTO_PROTO_15: __wasi_sockproto_t = 15;
    pub const __WASI_SOCK_PROTO_PROTO_16: __wasi_sockproto_t = 16;
    pub const __WASI_SOCK_PROTO_UDP: __wasi_sockproto_t = 17;
    pub const __WASI_SOCK_PROTO_PROTO_18: __wasi_sockproto_t = 18;
    pub const __WASI_SOCK_PROTO_PROTO_19: __wasi_sockproto_t = 19;
    pub const __WASI_SOCK_PROTO_PROTO_20: __wasi_sockproto_t = 20;
    pub const __WASI_SOCK_PROTO_PROTO_21: __wasi_sockproto_t = 21;
    pub const __WASI_SOCK_PROTO_IDP: __wasi_sockproto_t = 22;
    pub const __WASI_SOCK_PROTO_PROTO_23: __wasi_sockproto_t = 23;
    pub const __WASI_SOCK_PROTO_PROTO_24: __wasi_sockproto_t = 24;
    pub const __WASI_SOCK_PROTO_PROTO_25: __wasi_sockproto_t = 25;
    pub const __WASI_SOCK_PROTO_PROTO_26: __wasi_sockproto_t = 26;
    pub const __WASI_SOCK_PROTO_PROTO_27: __wasi_sockproto_t = 27;
    pub const __WASI_SOCK_PROTO_PROTO_28: __wasi_sockproto_t = 28;
    pub const __WASI_SOCK_PROTO_PROTO_TP: __wasi_sockproto_t = 29;
    pub const __WASI_SOCK_PROTO_PROTO_30: __wasi_sockproto_t = 30;
    pub const __WASI_SOCK_PROTO_PROTO_31: __wasi_sockproto_t = 31;
    pub const __WASI_SOCK_PROTO_PROTO_32: __wasi_sockproto_t = 32;
    pub const __WASI_SOCK_PROTO_DCCP: __wasi_sockproto_t = 33;
    pub const __WASI_SOCK_PROTO_PROTO_34: __wasi_sockproto_t = 34;
    pub const __WASI_SOCK_PROTO_PROTO_35: __wasi_sockproto_t = 35;
    pub const __WASI_SOCK_PROTO_PROTO_36: __wasi_sockproto_t = 36;
    pub const __WASI_SOCK_PROTO_PROTO_37: __wasi_sockproto_t = 37;
    pub const __WASI_SOCK_PROTO_PROTO_38: __wasi_sockproto_t = 38;
    pub const __WASI_SOCK_PROTO_PROTO_39: __wasi_sockproto_t = 39;
    pub const __WASI_SOCK_PROTO_PROTO_40: __wasi_sockproto_t = 40;
    pub const __WASI_SOCK_PROTO_IPV6: __wasi_sockproto_t = 41;
    pub const __WASI_SOCK_PROTO_PROTO_42: __wasi_sockproto_t = 42;
    pub const __WASI_SOCK_PROTO_ROUTING: __wasi_sockproto_t = 43;
    pub const __WASI_SOCK_PROTO_FRAGMENT: __wasi_sockproto_t = 44;
    pub const __WASI_SOCK_PROTO_PROTO_45: __wasi_sockproto_t = 45;
    pub const __WASI_SOCK_PROTO_RSVP: __wasi_sockproto_t = 46;
    pub const __WASI_SOCK_PROTO_GRE: __wasi_sockproto_t = 47;
    pub const __WASI_SOCK_PROTO_PROTO_48: __wasi_sockproto_t = 48;
    pub const __WASI_SOCK_PROTO_PROTO_49: __wasi_sockproto_t = 49;
    pub const __WASI_SOCK_PROTO_ESP: __wasi_sockproto_t = 50;
    pub const __WASI_SOCK_PROTO_AH: __wasi_sockproto_t = 51;
    pub const __WASI_SOCK_PROTO_PROTO_52: __wasi_sockproto_t = 52;
    pub const __WASI_SOCK_PROTO_PROTO_53: __wasi_sockproto_t = 53;
    pub const __WASI_SOCK_PROTO_PROTO_54: __wasi_sockproto_t = 54;
    pub const __WASI_SOCK_PROTO_PROTO_55: __wasi_sockproto_t = 55;
    pub const __WASI_SOCK_PROTO_PROTO_56: __wasi_sockproto_t = 56;
    pub const __WASI_SOCK_PROTO_PROTO_57: __wasi_sockproto_t = 57;
    pub const __WASI_SOCK_PROTO_ICMPV6: __wasi_sockproto_t = 58;
    pub const __WASI_SOCK_PROTO_NONE: __wasi_sockproto_t = 59;
    pub const __WASI_SOCK_PROTO_DSTOPTS: __wasi_sockproto_t = 60;
    pub const __WASI_SOCK_PROTO_PROTO_61: __wasi_sockproto_t = 61;
    pub const __WASI_SOCK_PROTO_PROTO_62: __wasi_sockproto_t = 62;
    pub const __WASI_SOCK_PROTO_PROTO_63: __wasi_sockproto_t = 63;
    pub const __WASI_SOCK_PROTO_PROTO_64: __wasi_sockproto_t = 64;
    pub const __WASI_SOCK_PROTO_PROTO_65: __wasi_sockproto_t = 65;
    pub const __WASI_SOCK_PROTO_PROTO_66: __wasi_sockproto_t = 66;
    pub const __WASI_SOCK_PROTO_PROTO_67: __wasi_sockproto_t = 67;
    pub const __WASI_SOCK_PROTO_PROTO_68: __wasi_sockproto_t = 68;
    pub const __WASI_SOCK_PROTO_PROTO_69: __wasi_sockproto_t = 69;
    pub const __WASI_SOCK_PROTO_PROTO_70: __wasi_sockproto_t = 70;
    pub const __WASI_SOCK_PROTO_PROTO_71: __wasi_sockproto_t = 71;
    pub const __WASI_SOCK_PROTO_PROTO_72: __wasi_sockproto_t = 72;
    pub const __WASI_SOCK_PROTO_PROTO_73: __wasi_sockproto_t = 73;
    pub const __WASI_SOCK_PROTO_PROTO_74: __wasi_sockproto_t = 74;
    pub const __WASI_SOCK_PROTO_PROTO_75: __wasi_sockproto_t = 75;
    pub const __WASI_SOCK_PROTO_PROTO_76: __wasi_sockproto_t = 76;
    pub const __WASI_SOCK_PROTO_PROTO_77: __wasi_sockproto_t = 77;
    pub const __WASI_SOCK_PROTO_PROTO_78: __wasi_sockproto_t = 78;
    pub const __WASI_SOCK_PROTO_PROTO_79: __wasi_sockproto_t = 79;
    pub const __WASI_SOCK_PROTO_PROTO_80: __wasi_sockproto_t = 80;
    pub const __WASI_SOCK_PROTO_PROTO_81: __wasi_sockproto_t = 81;
    pub const __WASI_SOCK_PROTO_PROTO_82: __wasi_sockproto_t = 82;
    pub const __WASI_SOCK_PROTO_PROTO_83: __wasi_sockproto_t = 83;
    pub const __WASI_SOCK_PROTO_PROTO_84: __wasi_sockproto_t = 84;
    pub const __WASI_SOCK_PROTO_PROTO_85: __wasi_sockproto_t = 85;
    pub const __WASI_SOCK_PROTO_PROTO_86: __wasi_sockproto_t = 86;
    pub const __WASI_SOCK_PROTO_PROTO_87: __wasi_sockproto_t = 87;
    pub const __WASI_SOCK_PROTO_PROTO_88: __wasi_sockproto_t = 88;
    pub const __WASI_SOCK_PROTO_PROTO_89: __wasi_sockproto_t = 89;
    pub const __WASI_SOCK_PROTO_PROTO_90: __wasi_sockproto_t = 90;
    pub const __WASI_SOCK_PROTO_PROTO_91: __wasi_sockproto_t = 91;
    pub const __WASI_SOCK_PROTO_MTP: __wasi_sockproto_t = 92;
    pub const __WASI_SOCK_PROTO_PROTO_93: __wasi_sockproto_t = 93;
    pub const __WASI_SOCK_PROTO_BEETPH: __wasi_sockproto_t = 94;
    pub const __WASI_SOCK_PROTO_PROTO_95: __wasi_sockproto_t = 95;
    pub const __WASI_SOCK_PROTO_PROTO_96: __wasi_sockproto_t = 96;
    pub const __WASI_SOCK_PROTO_PROTO_97: __wasi_sockproto_t = 97;
    pub const __WASI_SOCK_PROTO_ENCAP: __wasi_sockproto_t = 98;
    pub const __WASI_SOCK_PROTO_PROTO_99: __wasi_sockproto_t = 99;
    pub const __WASI_SOCK_PROTO_PROTO_100: __wasi_sockproto_t = 100;
    pub const __WASI_SOCK_PROTO_PROTO_101: __wasi_sockproto_t = 101;
    pub const __WASI_SOCK_PROTO_PROTO_102: __wasi_sockproto_t = 102;
    pub const __WASI_SOCK_PROTO_PIM: __wasi_sockproto_t = 103;
    pub const __WASI_SOCK_PROTO_PROTO_104: __wasi_sockproto_t = 104;
    pub const __WASI_SOCK_PROTO_PROTO_105: __wasi_sockproto_t = 105;
    pub const __WASI_SOCK_PROTO_PROTO_106: __wasi_sockproto_t = 106;
    pub const __WASI_SOCK_PROTO_PROTO_107: __wasi_sockproto_t = 107;
    pub const __WASI_SOCK_PROTO_COMP: __wasi_sockproto_t = 108;
    pub const __WASI_SOCK_PROTO_PROTO_109: __wasi_sockproto_t = 109;
    pub const __WASI_SOCK_PROTO_PROTO_110: __wasi_sockproto_t = 110;
    pub const __WASI_SOCK_PROTO_PROTO_111: __wasi_sockproto_t = 111;
    pub const __WASI_SOCK_PROTO_PROTO_112: __wasi_sockproto_t = 112;
    pub const __WASI_SOCK_PROTO_PROTO_113: __wasi_sockproto_t = 113;
    pub const __WASI_SOCK_PROTO_PROTO_114: __wasi_sockproto_t = 114;
    pub const __WASI_SOCK_PROTO_PROTO_115: __wasi_sockproto_t = 115;
    pub const __WASI_SOCK_PROTO_PROTO_116: __wasi_sockproto_t = 116;
    pub const __WASI_SOCK_PROTO_PROTO_117: __wasi_sockproto_t = 117;
    pub const __WASI_SOCK_PROTO_PROTO_118: __wasi_sockproto_t = 118;
    pub const __WASI_SOCK_PROTO_PROTO_119: __wasi_sockproto_t = 119;
    pub const __WASI_SOCK_PROTO_PROTO_120: __wasi_sockproto_t = 120;
    pub const __WASI_SOCK_PROTO_PROTO_121: __wasi_sockproto_t = 121;
    pub const __WASI_SOCK_PROTO_PROTO_122: __wasi_sockproto_t = 122;
    pub const __WASI_SOCK_PROTO_PROTO_123: __wasi_sockproto_t = 123;
    pub const __WASI_SOCK_PROTO_PROTO_124: __wasi_sockproto_t = 124;
    pub const __WASI_SOCK_PROTO_PROTO_125: __wasi_sockproto_t = 125;
    pub const __WASI_SOCK_PROTO_PROTO_126: __wasi_sockproto_t = 126;
    pub const __WASI_SOCK_PROTO_PROTO_127: __wasi_sockproto_t = 127;
    pub const __WASI_SOCK_PROTO_PROTO_128: __wasi_sockproto_t = 128;
    pub const __WASI_SOCK_PROTO_PROTO_129: __wasi_sockproto_t = 129;
    pub const __WASI_SOCK_PROTO_PROTO_130: __wasi_sockproto_t = 130;
    pub const __WASI_SOCK_PROTO_PROTO_131: __wasi_sockproto_t = 131;
    pub const __WASI_SOCK_PROTO_SCTP: __wasi_sockproto_t = 132;
    pub const __WASI_SOCK_PROTO_PROTO_133: __wasi_sockproto_t = 133;
    pub const __WASI_SOCK_PROTO_PROTO_134: __wasi_sockproto_t = 134;
    pub const __WASI_SOCK_PROTO_MH: __wasi_sockproto_t = 135;
    pub const __WASI_SOCK_PROTO_UDPLITE: __wasi_sockproto_t = 136;
    pub const __WASI_SOCK_PROTO_MPLS: __wasi_sockproto_t = 137;
    pub const __WASI_SOCK_PROTO_PROTO_138: __wasi_sockproto_t = 138;
    pub const __WASI_SOCK_PROTO_PROTO_139: __wasi_sockproto_t = 139;
    pub const __WASI_SOCK_PROTO_PROTO_140: __wasi_sockproto_t = 140;
    pub const __WASI_SOCK_PROTO_PROTO_141: __wasi_sockproto_t = 141;
    pub const __WASI_SOCK_PROTO_PROTO_142: __wasi_sockproto_t = 142;
    pub const __WASI_SOCK_PROTO_ETHERNET: __wasi_sockproto_t = 143;
    pub const __WASI_SOCK_PROTO_PROTO_144: __wasi_sockproto_t = 144;
    pub const __WASI_SOCK_PROTO_PROTO_145: __wasi_sockproto_t = 145;
    pub const __WASI_SOCK_PROTO_PROTO_146: __wasi_sockproto_t = 146;
    pub const __WASI_SOCK_PROTO_PROTO_147: __wasi_sockproto_t = 147;
    pub const __WASI_SOCK_PROTO_PROTO_148: __wasi_sockproto_t = 148;
    pub const __WASI_SOCK_PROTO_PROTO_149: __wasi_sockproto_t = 149;
    pub const __WASI_SOCK_PROTO_PROTO_150: __wasi_sockproto_t = 150;
    pub const __WASI_SOCK_PROTO_PROTO_151: __wasi_sockproto_t = 151;
    pub const __WASI_SOCK_PROTO_PROTO_152: __wasi_sockproto_t = 152;
    pub const __WASI_SOCK_PROTO_PROTO_153: __wasi_sockproto_t = 153;
    pub const __WASI_SOCK_PROTO_PROTO_154: __wasi_sockproto_t = 154;
    pub const __WASI_SOCK_PROTO_PROTO_155: __wasi_sockproto_t = 155;
    pub const __WASI_SOCK_PROTO_PROTO_156: __wasi_sockproto_t = 156;
    pub const __WASI_SOCK_PROTO_PROTO_157: __wasi_sockproto_t = 157;
    pub const __WASI_SOCK_PROTO_PROTO_158: __wasi_sockproto_t = 158;
    pub const __WASI_SOCK_PROTO_PROTO_159: __wasi_sockproto_t = 159;
    pub const __WASI_SOCK_PROTO_PROTO_160: __wasi_sockproto_t = 160;
    pub const __WASI_SOCK_PROTO_PROTO_161: __wasi_sockproto_t = 161;
    pub const __WASI_SOCK_PROTO_PROTO_162: __wasi_sockproto_t = 162;
    pub const __WASI_SOCK_PROTO_PROTO_163: __wasi_sockproto_t = 163;
    pub const __WASI_SOCK_PROTO_PROTO_164: __wasi_sockproto_t = 164;
    pub const __WASI_SOCK_PROTO_PROTO_165: __wasi_sockproto_t = 165;
    pub const __WASI_SOCK_PROTO_PROTO_166: __wasi_sockproto_t = 166;
    pub const __WASI_SOCK_PROTO_PROTO_167: __wasi_sockproto_t = 167;
    pub const __WASI_SOCK_PROTO_PROTO_168: __wasi_sockproto_t = 168;
    pub const __WASI_SOCK_PROTO_PROTO_169: __wasi_sockproto_t = 169;
    pub const __WASI_SOCK_PROTO_PROTO_170: __wasi_sockproto_t = 170;
    pub const __WASI_SOCK_PROTO_PROTO_171: __wasi_sockproto_t = 171;
    pub const __WASI_SOCK_PROTO_PROTO_172: __wasi_sockproto_t = 172;
    pub const __WASI_SOCK_PROTO_PROTO_173: __wasi_sockproto_t = 173;
    pub const __WASI_SOCK_PROTO_PROTO_174: __wasi_sockproto_t = 174;
    pub const __WASI_SOCK_PROTO_PROTO_175: __wasi_sockproto_t = 175;
    pub const __WASI_SOCK_PROTO_PROTO_176: __wasi_sockproto_t = 176;
    pub const __WASI_SOCK_PROTO_PROTO_177: __wasi_sockproto_t = 177;
    pub const __WASI_SOCK_PROTO_PROTO_178: __wasi_sockproto_t = 178;
    pub const __WASI_SOCK_PROTO_PROTO_179: __wasi_sockproto_t = 179;
    pub const __WASI_SOCK_PROTO_PROTO_180: __wasi_sockproto_t = 180;
    pub const __WASI_SOCK_PROTO_PROTO_181: __wasi_sockproto_t = 181;
    pub const __WASI_SOCK_PROTO_PROTO_182: __wasi_sockproto_t = 182;
    pub const __WASI_SOCK_PROTO_PROTO_183: __wasi_sockproto_t = 183;
    pub const __WASI_SOCK_PROTO_PROTO_184: __wasi_sockproto_t = 184;
    pub const __WASI_SOCK_PROTO_PROTO_185: __wasi_sockproto_t = 185;
    pub const __WASI_SOCK_PROTO_PROTO_186: __wasi_sockproto_t = 186;
    pub const __WASI_SOCK_PROTO_PROTO_187: __wasi_sockproto_t = 187;
    pub const __WASI_SOCK_PROTO_PROTO_188: __wasi_sockproto_t = 188;
    pub const __WASI_SOCK_PROTO_PROTO_189: __wasi_sockproto_t = 189;
    pub const __WASI_SOCK_PROTO_PROTO_190: __wasi_sockproto_t = 190;
    pub const __WASI_SOCK_PROTO_PROTO_191: __wasi_sockproto_t = 191;
    pub const __WASI_SOCK_PROTO_PROTO_192: __wasi_sockproto_t = 192;
    pub const __WASI_SOCK_PROTO_PROTO_193: __wasi_sockproto_t = 193;
    pub const __WASI_SOCK_PROTO_PROTO_194: __wasi_sockproto_t = 194;
    pub const __WASI_SOCK_PROTO_PROTO_195: __wasi_sockproto_t = 195;
    pub const __WASI_SOCK_PROTO_PROTO_196: __wasi_sockproto_t = 196;
    pub const __WASI_SOCK_PROTO_PROTO_197: __wasi_sockproto_t = 197;
    pub const __WASI_SOCK_PROTO_PROTO_198: __wasi_sockproto_t = 198;
    pub const __WASI_SOCK_PROTO_PROTO_199: __wasi_sockproto_t = 199;
    pub const __WASI_SOCK_PROTO_PROTO_200: __wasi_sockproto_t = 200;
    pub const __WASI_SOCK_PROTO_PROTO_201: __wasi_sockproto_t = 201;
    pub const __WASI_SOCK_PROTO_PROTO_202: __wasi_sockproto_t = 202;
    pub const __WASI_SOCK_PROTO_PROTO_203: __wasi_sockproto_t = 203;
    pub const __WASI_SOCK_PROTO_PROTO_204: __wasi_sockproto_t = 204;
    pub const __WASI_SOCK_PROTO_PROTO_205: __wasi_sockproto_t = 205;
    pub const __WASI_SOCK_PROTO_PROTO_206: __wasi_sockproto_t = 206;
    pub const __WASI_SOCK_PROTO_PROTO_207: __wasi_sockproto_t = 207;
    pub const __WASI_SOCK_PROTO_PROTO_208: __wasi_sockproto_t = 208;
    pub const __WASI_SOCK_PROTO_PROTO_209: __wasi_sockproto_t = 209;
    pub const __WASI_SOCK_PROTO_PROTO_210: __wasi_sockproto_t = 210;
    pub const __WASI_SOCK_PROTO_PROTO_211: __wasi_sockproto_t = 211;
    pub const __WASI_SOCK_PROTO_PROTO_212: __wasi_sockproto_t = 212;
    pub const __WASI_SOCK_PROTO_PROTO_213: __wasi_sockproto_t = 213;
    pub const __WASI_SOCK_PROTO_PROTO_214: __wasi_sockproto_t = 214;
    pub const __WASI_SOCK_PROTO_PROTO_215: __wasi_sockproto_t = 215;
    pub const __WASI_SOCK_PROTO_PROTO_216: __wasi_sockproto_t = 216;
    pub const __WASI_SOCK_PROTO_PROTO_217: __wasi_sockproto_t = 217;
    pub const __WASI_SOCK_PROTO_PROTO_218: __wasi_sockproto_t = 218;
    pub const __WASI_SOCK_PROTO_PROTO_219: __wasi_sockproto_t = 219;
    pub const __WASI_SOCK_PROTO_PROTO_220: __wasi_sockproto_t = 220;
    pub const __WASI_SOCK_PROTO_PROTO_221: __wasi_sockproto_t = 221;
    pub const __WASI_SOCK_PROTO_PROTO_222: __wasi_sockproto_t = 222;
    pub const __WASI_SOCK_PROTO_PROTO_223: __wasi_sockproto_t = 223;
    pub const __WASI_SOCK_PROTO_PROTO_224: __wasi_sockproto_t = 224;
    pub const __WASI_SOCK_PROTO_PROTO_225: __wasi_sockproto_t = 225;
    pub const __WASI_SOCK_PROTO_PROTO_226: __wasi_sockproto_t = 226;
    pub const __WASI_SOCK_PROTO_PROTO_227: __wasi_sockproto_t = 227;
    pub const __WASI_SOCK_PROTO_PROTO_228: __wasi_sockproto_t = 228;
    pub const __WASI_SOCK_PROTO_PROTO_229: __wasi_sockproto_t = 229;
    pub const __WASI_SOCK_PROTO_PROTO_230: __wasi_sockproto_t = 230;
    pub const __WASI_SOCK_PROTO_PROTO_231: __wasi_sockproto_t = 231;
    pub const __WASI_SOCK_PROTO_PROTO_232: __wasi_sockproto_t = 232;
    pub const __WASI_SOCK_PROTO_PROTO_233: __wasi_sockproto_t = 233;
    pub const __WASI_SOCK_PROTO_PROTO_234: __wasi_sockproto_t = 234;
    pub const __WASI_SOCK_PROTO_PROTO_235: __wasi_sockproto_t = 235;
    pub const __WASI_SOCK_PROTO_PROTO_236: __wasi_sockproto_t = 236;
    pub const __WASI_SOCK_PROTO_PROTO_237: __wasi_sockproto_t = 237;
    pub const __WASI_SOCK_PROTO_PROTO_238: __wasi_sockproto_t = 238;
    pub const __WASI_SOCK_PROTO_PROTO_239: __wasi_sockproto_t = 239;
    pub const __WASI_SOCK_PROTO_PROTO_240: __wasi_sockproto_t = 240;
    pub const __WASI_SOCK_PROTO_PROTO_241: __wasi_sockproto_t = 241;
    pub const __WASI_SOCK_PROTO_PROTO_242: __wasi_sockproto_t = 242;
    pub const __WASI_SOCK_PROTO_PROTO_243: __wasi_sockproto_t = 243;
    pub const __WASI_SOCK_PROTO_PROTO_244: __wasi_sockproto_t = 244;
    pub const __WASI_SOCK_PROTO_PROTO_245: __wasi_sockproto_t = 245;
    pub const __WASI_SOCK_PROTO_PROTO_246: __wasi_sockproto_t = 246;
    pub const __WASI_SOCK_PROTO_PROTO_247: __wasi_sockproto_t = 247;
    pub const __WASI_SOCK_PROTO_PROTO_248: __wasi_sockproto_t = 248;
    pub const __WASI_SOCK_PROTO_PROTO_249: __wasi_sockproto_t = 249;
    pub const __WASI_SOCK_PROTO_PROTO_250: __wasi_sockproto_t = 250;
    pub const __WASI_SOCK_PROTO_PROTO_251: __wasi_sockproto_t = 251;
    pub const __WASI_SOCK_PROTO_PROTO_252: __wasi_sockproto_t = 252;
    pub const __WASI_SOCK_PROTO_PROTO_253: __wasi_sockproto_t = 253;
    pub const __WASI_SOCK_PROTO_PROTO_254: __wasi_sockproto_t = 254;
    pub const __WASI_SOCK_PROTO_PROTO_RAW: __wasi_sockproto_t = 255;
    pub const __WASI_SOCK_PROTO_PROTO_256: __wasi_sockproto_t = 256;
    pub const __WASI_SOCK_PROTO_PROTO_257: __wasi_sockproto_t = 257;
    pub const __WASI_SOCK_PROTO_PROTO_258: __wasi_sockproto_t = 258;
    pub const __WASI_SOCK_PROTO_PROTO_259: __wasi_sockproto_t = 259;
    pub const __WASI_SOCK_PROTO_PROTO_260: __wasi_sockproto_t = 260;
    pub const __WASI_SOCK_PROTO_PROTO_261: __wasi_sockproto_t = 261;
    pub const __WASI_SOCK_PROTO_MPTCP: __wasi_sockproto_t = 262;
    pub const __WASI_SOCK_PROTO_MAX: __wasi_sockproto_t = 263;

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
        pub preferred_until: __wasi_option_timestamp_t,
        pub expires_at: __wasi_option_timestamp_t,
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
        pub ok: __wasi_bool_t,
        pub redirect: __wasi_bool_t,
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
    use super::io::__wasi_option_t;
    use wasmer_derive::ValueType;
    use wasmer_wasi_types_generated::wasi::Timestamp;

    #[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
    #[repr(C)]
    pub struct __wasi_option_timestamp_t {
        pub tag: __wasi_option_t,
        pub u: Timestamp,
    }
}
