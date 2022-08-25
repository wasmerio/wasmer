use std::{
    fmt,
    mem::{self, MaybeUninit},
};
use wasmer_derive::ValueType;
use wasmer_types::ValueType;
use wasmer_wasi_types_generated::wasi::{Fd, Preopentype, Rights};

pub const __WASI_STDIN_FILENO: Fd = 0;
pub const __WASI_STDOUT_FILENO: Fd = 1;
pub const __WASI_STDERR_FILENO: Fd = 2;

pub type __wasi_pid_t = u32;
pub type __wasi_tid_t = u32;

pub type __wasi_eventfdflags = u16;
pub const __WASI_EVENTFDFLAGS_SEMAPHORE: __wasi_eventfdflags = 1 << 0;

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_prestat_u_dir_t {
    pub pr_name_len: u32,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub union __wasi_prestat_u {
    dir: __wasi_prestat_u_dir_t,
}

impl fmt::Debug for __wasi_prestat_u {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "__wasi_prestat_u")
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct __wasi_prestat_t {
    pub pr_type: Preopentype,
    pub u: __wasi_prestat_u,
}

#[derive(Copy, Clone)]
pub enum PrestatEnum {
    Dir { pr_name_len: u32 },
}

impl PrestatEnum {
    pub fn untagged(self) -> __wasi_prestat_u {
        match self {
            PrestatEnum::Dir { pr_name_len } => __wasi_prestat_u {
                dir: __wasi_prestat_u_dir_t { pr_name_len },
            },
        }
    }
}

impl __wasi_prestat_t {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn tagged(&self) -> Option<PrestatEnum> {
        match self.pr_type {
            Preopentype::Dir => Some(PrestatEnum::Dir {
                pr_name_len: unsafe { self.u.dir.pr_name_len },
            }),
        }
    }
}

unsafe impl ValueType for __wasi_prestat_t {
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
        self.pr_type
            .zero_padding_bytes(&mut bytes[field!(pr_type)..field_end!(pr_type)]);
        zero!(field_end!(pr_type), field!(u));
        match self.pr_type {
            Preopentype::Dir => unsafe {
                self.u
                    .dir
                    .zero_padding_bytes(&mut bytes[field!(u.dir)..field_end!(u.dir)]);
                zero!(field_end!(u.dir), field_end!(u));
            },
        }
        zero!(field_end!(u), mem::size_of_val(self));
    }
}

pub type __wasi_filedelta_t = i64;

pub type __wasi_fstflags_t = u16;
pub const __WASI_FILESTAT_SET_ATIM: __wasi_fstflags_t = 1 << 0;
pub const __WASI_FILESTAT_SET_ATIM_NOW: __wasi_fstflags_t = 1 << 1;
pub const __WASI_FILESTAT_SET_MTIM: __wasi_fstflags_t = 1 << 2;
pub const __WASI_FILESTAT_SET_MTIM_NOW: __wasi_fstflags_t = 1 << 3;

pub type __wasi_lookupflags_t = u32;
pub const __WASI_LOOKUP_SYMLINK_FOLLOW: __wasi_lookupflags_t = 1 << 0;

pub type __wasi_oflags_t = u16;
pub const __WASI_O_CREAT: __wasi_oflags_t = 1 << 0;
pub const __WASI_O_DIRECTORY: __wasi_oflags_t = 1 << 1;
pub const __WASI_O_EXCL: __wasi_oflags_t = 1 << 2;
pub const __WASI_O_TRUNC: __wasi_oflags_t = 1 << 3;

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
