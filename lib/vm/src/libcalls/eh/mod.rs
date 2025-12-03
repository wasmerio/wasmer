// Part of the logic, here, is borrowed as-is from rust's stdlib.

use crate::{InternalStoreHandle, VMContext, VMExceptionObj};

mod dwarf;

cfg_if::cfg_if! {
    if #[cfg(any(target_env = "msvc", target_family = "wasm"))] {
        /// The implementation of Wasmer's personality function.
        ///
        /// # Safety
        ///
        /// Performs libunwind unwinding magic.
        pub unsafe fn wasmer_eh_personality() {
            panic!()
        }

        /// The second stage of the personality function. See module level documentation
        /// for an explanation of the exact procedure used during unwinding.
        ///
        /// # Safety
        ///
        /// Does pointer accesses, which must be valid.
        pub unsafe fn wasmer_eh_personality2() {
            panic!()
        }

        pub unsafe fn read_exnref(_exception: *mut std::ffi::c_void) -> u32 {
            panic!()
        }

        /// # Safety
        ///
        /// Performs libunwind unwinding magic. Highly unsafe.
        pub unsafe fn throw(_ctx: &crate::StoreObjects, _exnref: u32) -> ! {
            panic!()
        }

        /// Given a pointer to a caught exception, return the exnref contained within.
        ///
        /// # Safety
        ///
        /// `exception` must be a pointer the platform-specific exception type; this is
        /// `UwExceptionWrapper` for gcc.
        pub unsafe fn delete_exception(_exception: *mut std::ffi::c_void) {
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

pub(crate) fn exn_obj_from_exnref(vmctx: *mut VMContext, exnref: u32) -> *mut VMExceptionObj {
    let instance = unsafe { (*vmctx).instance_mut() };
    let exnref = InternalStoreHandle::<VMExceptionObj>::from_index(exnref as usize).unwrap();
    let exn = exnref.get_mut(instance.context_mut());
    exn as *mut VMExceptionObj
}
