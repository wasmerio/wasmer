// Part of the logic, here, is borrowed as-is from rust's stdlib.

mod dwarf;

cfg_if::cfg_if! {
    if #[cfg(any(target_env = "msvc", target_family = "wasm"))] {
        // We have yet to figure this out.
        #[repr(C)]
        pub struct UwExceptionWrapper {
            pub _uwe: (),
            pub cause: Box<dyn std::any::Any + Send>,
        }

        impl UwExceptionWrapper {
            pub fn new(tag: u64, data_ptr: usize, data_size: u64) -> Self {
                Self {
                    _uwe: (),
                    cause: Box::new(WasmerException {
                        tag,
                        data_ptr,
                        data_size,
                    }),
                }
            }
        }

        #[repr(C)]
        #[derive(Debug, thiserror::Error, Clone)]
        #[error("Uncaught exception in wasm code!")]
        pub struct WasmerException {
            pub tag: u64,
            pub data_ptr: usize,
            pub data_size: u64,
        }

        pub fn wasmer_eh_personality() {
            panic!()
        }

        pub  fn throw(tag: u64, data_ptr: usize, data_size: u64) -> ! {
            panic!()
        }

        pub fn rethrow(exc: *mut UwExceptionWrapper) -> ! {
            panic!()
        }


    } else if #[cfg(any(
        all(target_family = "windows", target_env = "gnu"),
        target_family = "unix",
    ))] {
        // gcc-like eh-personality mechanisms.
        mod gcc;
        pub use gcc::*;
    } else {
        // Targets that don't support unwinding.
        // - os=none ("bare metal" targets)
        // - os=uefi
        // - os=espidf
        // - os=hermit
        // - nvptx64-nvidia-cuda
        // - arch=avr
    }
}
