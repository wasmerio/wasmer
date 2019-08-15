// Args:
// mapdir: hamlet:wasitests/test_fs/hamlet

use std::fs;
use std::io::{Read, Seek, SeekFrom};
#[cfg(target_os = "wasi")]
use std::os::wasi::prelude::AsRawFd;
use std::path::PathBuf;

#[cfg(target_os = "wasi")]
#[link(wasm_import_module = "wasi_unstable")]
extern "C" {
    fn poll_oneoff(subscriptons: u32, events: u32, nsubscriptons: u32, nevents: u32) -> u16;
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct __wasi_event_t {
    pub userdata: u64,
    pub error: u16,
    pub type_: __wasi_eventtype_t,
    pub u: __wasi_event_u,
}

impl Default for __wasi_event_t {
    fn default() -> Self {
        __wasi_event_t {
            userdata: 0,
            error: 0,
            type_: 0,
            u: __wasi_event_u {
                fd_readwrite: __wasi_event_fd_readwrite_t {
                    nbytes: 0,
                    flags: 0,
                },
            },
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct __wasi_event_fd_readwrite_t {
    pub nbytes: u64,
    pub flags: u16,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub union __wasi_event_u {
    pub fd_readwrite: __wasi_event_fd_readwrite_t,
}

impl std::fmt::Debug for __wasi_event_u {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "__wasi_event_u {{ ")?;
        write!(f, "{:?} }}", unsafe { self.fd_readwrite })
    }
}

pub type __wasi_eventtype_t = u8;
pub const __WASI_EVENTTYPE_CLOCK: u8 = 0;
pub const __WASI_EVENTTYPE_FD_READ: u8 = 1;
pub const __WASI_EVENTTYPE_FD_WRITE: u8 = 2;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct __wasi_subscription_clock_t {
    pub userdata: u64,
    pub clock_id: u32,
    pub timeout: u64,
    pub precision: u64,
    pub flags: u16,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct __wasi_subscription_fs_readwrite_t {
    pub fd: u32,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub union __wasi_subscription_u {
    clock: __wasi_subscription_clock_t,
    fd_readwrite: __wasi_subscription_fs_readwrite_t,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct __wasi_subscription_t {
    pub userdata: u64,
    pub type_: __wasi_eventtype_t,
    pub u: __wasi_subscription_u,
}

#[cfg(target_os = "wasi")]
fn poll_read(fds: &[u32]) -> Result<Vec<__wasi_event_t>, u16> {
    let mut in_ = fds
        .iter()
        .map(|n| __wasi_subscription_t {
            userdata: 0x123456,
            type_: __WASI_EVENTTYPE_FD_READ,
            u: __wasi_subscription_u {
                fd_readwrite: __wasi_subscription_fs_readwrite_t { fd: *n as u32 },
            },
        })
        .collect::<Vec<_>>();
    let mut out_ = vec![Default::default(); in_.len()];
    let mut nsubscriptions: u32 = 0;
    let result = unsafe {
        poll_oneoff(
            in_.as_ptr() as usize as u32,
            out_.as_mut_ptr() as usize as u32,
            in_.len() as u32,
            (&mut nsubscriptions as *mut u32) as usize as u32,
        )
    };

    if result == 0 {
        Ok(out_)
    } else {
        Err(result)
    }
}

fn main() {
    #[cfg(not(target_os = "wasi"))]
    let mut base = PathBuf::from("wasitests/test_fs");
    #[cfg(target_os = "wasi")]
    let mut base = PathBuf::from("/");

    let path = base.join("hamlet/act4/scene1.txt");
    let mut file = fs::File::open(&path).expect("Could not open file");
    let mut buffer = [0u8; 64];

    #[cfg(target_os = "wasi")]
    {
        let fds = vec![file.as_raw_fd()];
        let mut result = poll_read(fds.as_slice()).expect("First poll");
        while result[0].error != 0 {
            result = poll_read(fds.as_slice()).expect("subsequent polls");
        }
        println!("{:?}", result[0]);
    }
    #[cfg(not(target_os = "wasi"))]
    {
        println!("{}", "__wasi_event_t { userdata: 1193046, error: 0, type_: 1, u: __wasi_event_u { __wasi_event_fd_readwrite_t { nbytes: 2259, flags: 0 } } }");
    }
}
