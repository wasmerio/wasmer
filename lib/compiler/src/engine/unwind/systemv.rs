// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

//! Module for System V ABI unwind registry.

use core::sync::atomic::{
    AtomicBool, AtomicUsize,
    Ordering::{self, Relaxed},
};
use std::sync::Once;

use gimli::{BaseAddresses, CieOrFde, EhFrame, NativeEndian, UnwindSection};

/// Represents a registry of function unwind information for System V ABI.
pub struct UnwindRegistry {
    published: bool,
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    registrations: Vec<usize>,
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
        // Register atexit handler that will tell us if exit has been called.
        static INIT: Once = Once::new();
        INIT.call_once(|| unsafe {
            let result = libc::atexit(atexit_handler);
            assert_eq!(result, 0, "libc::atexit must succeed");
        });
        assert!(
            !EXIT_CALLED.load(Ordering::SeqCst),
            "Cannot register unwind information during the process exit"
        );

        Self {
            published: false,
            #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
            registrations: Vec::new(),
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            compact_unwind_mgr: Default::default(),
        }
    }

    /// Publishes all registered functions (coming from .eh_frame sections).
    //#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    // TODO
    pub fn publish_eh_frame(&mut self, eh_frame: Option<&[u8]>) -> Result<(), String> {
        if self.published {
            return Err("unwind registry has already been published".to_string());
        }

        unsafe {
            // TODO
            // if let Some(eh_frame) = eh_frame {
            //     self.register_eh_frames(eh_frame)?;
            // }
        }

        self.published = true;

        Ok(())
    }

    #[allow(clippy::cast_ptr_alignment)]
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    unsafe fn register_eh_frames(&mut self, eh_frame: &[u8]) -> Result<(), String> {
        // Validate that the `.eh_frame` is well-formed before registering it.
        // See https://refspecs.linuxfoundation.org/LSB_3.0.0/LSB-Core-generic/LSB-Core-generic/ehframechpt.html for more details.
        // We use the gimli crate to parse and validate the section, because
        // calling `__register_frame` with invalid data can cause segfaults.
        let mut eh_frame_gimli = EhFrame::new(eh_frame, NativeEndian);
        eh_frame_gimli.set_address_size(size_of::<usize>() as u8);
        // Provide base addresses so gimli can resolve encoded pointers.
        // We don't need accurate resolved values — only structural validation.
        let bases = BaseAddresses::default()
            .set_eh_frame(eh_frame.as_ptr() as u64)
            .set_text(0)
            .set_got(0);
        let using_libunwind = using_libunwind();

        if using_libunwind {
            // For libunwind based systems, `__register_frame` takes a pointer to an
            // individual FDE.
            let mut entries = eh_frame_gimli.entries(&bases);
            while let Some(entry) = entries
                .next()
                .map_err(|e| format!("failed to parse .eh_frame entry: {e}"))?
            {
                if let CieOrFde::Fde(fde) = entry {
                    let offset = fde.offset();
                    let record = eh_frame.as_ptr() as usize + offset;
                    unsafe {
                        __register_frame(record as *const u8);
                    }
                    self.registrations.push(record);
                }
            }
        } else {
            // For libgcc based systems, `__register_frame` takes a pointer to the
            // beginning of the `.eh_frame` section, which is itself a
            // null-terminated list of CIEs and FDEs.

            // For debugging purposes, we can validate the section first by iterating all entries.
            //
            // let mut entries = eh_frame_gimli.entries(&bases);
            // while let Some(_entry) = entries.next().expect("failed to parse .eh_frame entry") {
            // }

            let record = eh_frame.as_ptr() as usize;
            unsafe {
                __register_frame(record as *const u8);
            }
            self.registrations.push(record);
        }

        Ok(())
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    pub(crate) fn publish_compact_unwind(
        &mut self,
        compact_unwind: &[u8],
        eh_personality_addr_in_got: Option<usize>,
    ) -> Result<(), String> {
        if self.published {
            return Err("unwind registry has already been published".to_string());
        }

        unsafe {
            self.compact_unwind_mgr.read_compact_unwind_section(
                compact_unwind.as_ptr() as _,
                compact_unwind.len(),
                eh_personality_addr_in_got,
            )?;
            self.compact_unwind_mgr
                .finalize()
                .map_err(|v| v.to_string())?;
            self.compact_unwind_mgr.register();
        }

        self.published = true;
        Ok(())
    }
}

/// Global flag indicating whether the program exit has been initiated.
/// Set to true by an atexit handler to prevent crashes during shutdown
/// when deregistering unwind frames. Accesses use `Ordering::SeqCst`
/// to ensure correct memory ordering across threads.
pub static EXIT_CALLED: AtomicBool = AtomicBool::new(false);

extern "C" fn atexit_handler() {
    EXIT_CALLED.store(true, Ordering::SeqCst);
}

impl Drop for UnwindRegistry {
    fn drop(&mut self) {
        if self.published {
            #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
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

                unsafe {
                    __deregister_frame(*registration as *const _);
                }
            }

            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            {
                if EXIT_CALLED.load(Ordering::SeqCst) {
                    return;
                }
                self.compact_unwind_mgr.deregister();
            }
        }
    }
}
