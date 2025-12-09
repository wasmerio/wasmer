// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

//! Module for System V ABI unwind registry.

use core::sync::atomic::{
    AtomicBool, AtomicUsize,
    Ordering::{self, Relaxed},
};
use std::sync::Once;

use crate::types::unwind::CompiledFunctionUnwindInfoReference;

/// Represents a registry of function unwind information for System V ABI.
pub struct UnwindRegistry {
    registrations: Vec<usize>,
    published: bool,
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    compact_unwind_mgr: compact_unwind::CompactUnwindManager,
}

unsafe extern "C" {
    // libunwind import
    fn __register_frame(fde: *const u8);
    fn __deregister_frame(fde: *const u8);
}

// Apple-specific unwind functions - the following is taken from LLVM's libunwind itself.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod compact_unwind;

/// There are two primary unwinders on Unix platforms: libunwind and libgcc.
///
/// Unfortunately their interface to `__register_frame` is different. The
/// libunwind library takes a pointer to an individual FDE while libgcc takes a
/// null-terminated list of FDEs. This means we need to know what unwinder
/// is being used at runtime.
///
/// This detection is done currently by looking for a libunwind-specific symbol.
/// This specific symbol was somewhat recommended by LLVM's
/// "RTDyldMemoryManager.cpp" file which says:
///
/// > We use the presence of __unw_add_dynamic_fde to detect libunwind.
///
/// I'll note that there's also a different libunwind project at
/// https://www.nongnu.org/libunwind/ but that doesn't appear to have
/// `__register_frame` so I don't think that interacts with this.
#[allow(dead_code)]
fn using_libunwind() -> bool {
    static USING_LIBUNWIND: AtomicUsize = AtomicUsize::new(LIBUNWIND_UNKNOWN);

    const LIBUNWIND_UNKNOWN: usize = 0;
    const LIBUNWIND_YES: usize = 1;
    const LIBUNWIND_NO: usize = 2;

    // On macOS the libgcc interface is never used so libunwind is always used.
    if cfg!(target_os = "macos") {
        return true;
    }

    // TODO: wasmtime started using weak symbol definition that makes the detection
    // more reliable on linux-musl target: https://github.com/bytecodealliance/wasmtime/pull/9479
    if cfg!(target_env = "musl") {
        return true;
    }

    // On other platforms the unwinder can vary. Sometimes the unwinder is
    // selected at build time and sometimes it differs at build time and runtime
    // (or at least I think that's possible). Fall back to a `libc::dlsym` to
    // figure out what we're using and branch based on that.
    //
    // Note that the result of `libc::dlsym` is cached to only look this up
    // once.
    match USING_LIBUNWIND.load(Relaxed) {
        LIBUNWIND_YES => true,
        LIBUNWIND_NO => false,
        LIBUNWIND_UNKNOWN => {
            let looks_like_libunwind = unsafe {
                !libc::dlsym(
                    std::ptr::null_mut(),
                    c"__unw_add_dynamic_fde".as_ptr().cast(),
                )
                .is_null()
            };
            USING_LIBUNWIND.store(
                if looks_like_libunwind {
                    LIBUNWIND_YES
                } else {
                    LIBUNWIND_NO
                },
                Relaxed,
            );
            looks_like_libunwind
        }
        _ => unreachable!(),
    }
}

impl UnwindRegistry {
    /// Creates a new unwind registry with the given base address.
    pub fn new() -> Self {
        Self {
            registrations: Vec::new(),
            published: false,
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            compact_unwind_mgr: Default::default(),
        }
    }

    /// Registers a function given the start offset, length, and unwind information.
    pub fn register(
        &mut self,
        _base_address: usize,
        _func_start: u32,
        _func_len: u32,
        info: &CompiledFunctionUnwindInfoReference,
    ) -> Result<(), String> {
        match info {
            CompiledFunctionUnwindInfoReference::Dwarf => {}
            _ => return Err(format!("unsupported unwind information {info:?}")),
        };
        Ok(())
    }

    /// Publishes all registered functions.
    pub fn publish(&mut self, eh_frame: Option<&[u8]>) -> Result<(), String> {
        if self.published {
            return Err("unwind registry has already been published".to_string());
        }

        if let Some(eh_frame) = eh_frame {
            unsafe {
                self.register_frames(eh_frame);
            }
        }

        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            self.compact_unwind_mgr
                .finalize()
                .map_err(|v| v.to_string())?;
            self.compact_unwind_mgr.register();
        }

        self.published = true;

        Ok(())
    }

    #[allow(clippy::cast_ptr_alignment)]
    unsafe fn register_frames(&mut self, eh_frame: &[u8]) {
        // Register atexit handler that will tell us if exit has been called.
        static INIT: Once = Once::new();
        INIT.call_once(|| unsafe {
            libc::atexit(atexit_handler);
        });

        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            // Special call for macOS on aarch64 to register the `.eh_frame` section.
            // TODO: I am not 100% sure if it's correct to never deregister the `.eh_frame` section. It was this way before
            // I started working on this, so I kept it that way.
            unsafe {
                compact_unwind::__unw_add_dynamic_eh_frame_section(eh_frame.as_ptr() as usize);
            }
        }

        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        {
            // Validate that the `.eh_frame` is well-formed before registering it.
            // See https://refspecs.linuxfoundation.org/LSB_3.0.0/LSB-Core-generic/LSB-Core-generic/ehframechpt.html for more details.
            // We put the frame records into a vector before registering them, because
            // calling `__register_frame` with invalid data can cause segfaults.

            // Pointers to the registrations that will be registered with `__register_frame`.
            // For libgcc based systems, these are CIEs.
            // For libunwind based systems, these are FDEs.
            let mut records_to_register = Vec::new();

            let mut current = 0;
            let mut last_len = 0;
            while current <= (eh_frame.len() - size_of::<u32>()) {
                // If a CFI or a FDE starts with 0u32 it is a terminator.
                let len = u32::from_ne_bytes(eh_frame[current..(current + 4)].try_into().unwrap());
                if len == 0 {
                    current += size_of::<u32>();
                    last_len = 0;
                    continue;
                }
                // The first record after a terminator is always a CIE.
                let is_cie = last_len == 0;
                last_len = len;
                let record = eh_frame.as_ptr() as usize + current;
                current = current + len as usize + 4;

                if using_libunwind() {
                    // For libunwind based systems, `__register_frame` takes a pointer to an FDE.
                    if !is_cie {
                        // Every record that's not a CIE is an FDE.
                        records_to_register.push(record);
                    }
                    continue;
                }

                // For libgcc based systems, `__register_frame` takes a pointer to a CIE.
                if is_cie {
                    records_to_register.push(record);
                }
            }

            assert_eq!(
                last_len, 0,
                "The last record in the `.eh_frame` must be a terminator (but it actually has length {last_len})"
            );
            assert_eq!(
                current,
                eh_frame.len(),
                "The `.eh_frame` must be finished after the last record",
            );

            for record in records_to_register {
                // Register the CFI with libgcc
                unsafe {
                    __register_frame(record as *const u8);
                }
                self.registrations.push(record);
            }
        }
    }

    pub(crate) fn register_compact_unwind(
        &mut self,
        compact_unwind: Option<&[u8]>,
        eh_personality_addr_in_got: Option<usize>,
    ) -> Result<(), String> {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        unsafe {
            if let Some(slice) = compact_unwind {
                self.compact_unwind_mgr.read_compact_unwind_section(
                    slice.as_ptr() as _,
                    slice.len(),
                    eh_personality_addr_in_got,
                )?;
            }
        }

        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        {
            _ = compact_unwind;
            _ = eh_personality_addr_in_got;
        }
        Ok(())
    }
}

pub static EXIT_CALLED: AtomicBool = AtomicBool::new(false);

extern "C" fn atexit_handler() {
    EXIT_CALLED.store(true, Ordering::SeqCst);
}

impl Drop for UnwindRegistry {
    fn drop(&mut self) {
        if self.published {
            unsafe {
                // libgcc stores the frame entries as a linked list in decreasing sort order
                // based on the PC value of the registered entry.
                //
                // As we store the registrations in increasing order, it would be O(N^2) to
                // deregister in that order.
                //
                // To ensure that we just pop off the first element in the list upon every
                // deregistration, walk our list of registrations backwards.
                for registration in self.registrations.iter().rev() {
                    // We don't want to deregister frames in UnwindRegistry::Drop as that could be called during
                    // program shutdown and can collide with release_registered_frames and lead to
                    // crashes.
                    if EXIT_CALLED.load(Ordering::SeqCst) {
                        return;
                    }

                    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
                    {
                        compact_unwind::__unw_remove_dynamic_eh_frame_section(*registration);
                        self.compact_unwind_mgr.deregister();
                    }

                    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
                    {
                        __deregister_frame(*registration as *const _);
                    }
                }
            }
        }
    }
}
