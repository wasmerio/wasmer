// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

//! Module for System V ABI unwind registry.

use core::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use crate::types::unwind::CompiledFunctionUnwindInfoReference;

/// Represents a registry of function unwind information for System V ABI.
pub struct UnwindRegistry {
    registrations: Vec<usize>,
    published: bool,
}

extern "C" {
    // libunwind import
    fn __register_frame(fde: *const u8);
    fn __deregister_frame(fde: *const u8);
}

// Apple-specific unwind functions - the following is taken from LLVM's libunwind itself.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod _apple_uw {
    use std::{
        collections::HashMap,
        sync::{LazyLock, Mutex},
    };

    static ADDRESSES_MAP: LazyLock<Mutex<HashMap<usize, usize>>> =
        LazyLock::new(|| Mutex::new(HashMap::default()));

    #[repr(C)]
    /// Holds a description of the object-format-header (if any) and unwind info
    /// sections for a given address:
    ///
    /// * dso_base should point to a header for the JIT'd object containing the
    ///   given address. The header's type should match the format type that
    ///   libunwind was compiled for (so a mach_header or mach_header_64 on Darwin).
    ///   A value of zero indicates that no such header exists.
    ///
    /// * dwarf_section and dwarf_section_length hold the address range of a DWARF
    ///   eh-frame section associated with the given address, if any. If the
    ///   dwarf_section_length field is zero it indicates that no such section
    ///   exists (and in this case dwarf_section should also be set to zero).
    ///
    /// * compact_unwind_section and compact_unwind_section_length hold the address
    ///   range of a compact-unwind info section associated with the given address,
    ///   if any. If the compact_unwind_section_length field is zero it indicates
    ///   that no such section exists (and in this case compact_unwind_section
    ///   should also be set to zero).
    #[derive(Debug)]
    pub struct UnwDynamicUnwindSections {
        dso_base: usize,
        dwarf_section: usize,
        dwarf_section_length: usize,
        compact_unwind_section: usize,
        compact_unwind_section_length: usize,
    }

    // Typedef for unwind-info lookup callbacks. Functions of this type can be
    // registered and deregistered using __unw_add_find_dynamic_unwind_sections
    // and __unw_remove_find_dynamic_unwind_sections respectively.
    //
    // An unwind-info lookup callback should return 1 to indicate that it found
    // unwind-info for the given address, or 0 to indicate that it did not find
    // unwind-info for the given address. If found, the callback should populate
    // some or all of the fields of the info argument (which is guaranteed to be
    // non-null with all fields zero-initialized):
    type UnwFindDynamicUnwindSections =
        unsafe extern "C" fn(addr: usize, info: *mut UnwDynamicUnwindSections) -> u32;

    unsafe extern "C" fn x(_addr: usize, _info: *mut UnwDynamicUnwindSections) -> u32 {
        todo!()
    }

    pub unsafe fn generate_find_dynamic_unwind_sections(
        ptr: usize,
        len: usize,
    ) -> UnwFindDynamicUnwindSections {
        let bytes = std::slice::from_raw_parts(std::mem::transmute::<usize, *const u8>(ptr), len);
        let p = macho_unwind_info::UnwindInfo::parse(bytes).unwrap();
        let mut funcs = p.functions();

        while let Ok(Some(f)) = funcs.next() {
            println!("{f:?}");
        }

        x
    }

    extern "C" {
        // Register a dynamic unwind-info lookup callback. If libunwind does not find
        // unwind info for a given frame in the executable program or normal dynamic
        // shared objects then it will call all registered dynamic lookup functions
        // in registration order until either one of them returns true, or the end
        // of the list is reached. This lookup will happen before libunwind searches
        // any eh-frames registered via __register_frame or
        // __unw_add_dynamic_eh_frame_section.
        //
        // Returns UNW_ESUCCESS for successful registrations. If the given callback
        // has already been registered then UNW_EINVAL will be returned. If all
        // available callback entries are in use then UNW_ENOMEM will be returned.
        pub fn __unw_add_find_dynamic_unwind_sections(
            find_dynamic_unwind_sections: UnwFindDynamicUnwindSections,
        ) -> u32;

        // Deregister a dynacim unwind-info lookup callback.
        //
        // Returns UNW_ESUCCESS for successful deregistrations. If the given callback
        // has already been registered then UNW_EINVAL will be returned.
        pub fn __unw_remove_find_dynamic_unwind_sections(
            find_dynamic_unwind_sections: &UnwDynamicUnwindSections,
        ) -> u32;

    }
}

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
fn using_libunwind() -> bool {
    static USING_LIBUNWIND: AtomicUsize = AtomicUsize::new(LIBUNWIND_UNKNOWN);

    const LIBUNWIND_UNKNOWN: usize = 0;
    const LIBUNWIND_YES: usize = 1;
    const LIBUNWIND_NO: usize = 2;

    // On macOS the libgcc interface is never used so libunwind is always used.
    if cfg!(target_os = "macos") {
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
                    "__unw_add_dynamic_fde\0".as_ptr().cast(),
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

        self.published = true;

        Ok(())
    }

    #[allow(clippy::cast_ptr_alignment)]
    unsafe fn register_frames(&mut self, eh_frame: &[u8]) {
        if !using_libunwind() {
            // Registering an empty `eh_frame` (i.e. which
            // contains empty FDEs) cause problems on Linux when
            // deregistering it. We must avoid this
            // scenario. Usually, this is handled upstream by the
            // compilers.
            debug_assert_ne!(
                eh_frame,
                &[0, 0, 0, 0],
                "`eh_frame` seems to contain empty FDEs"
            );

            // On gnu (libgcc), `__register_frame` will walk the FDEs until an entry of length 0
            let ptr = eh_frame.as_ptr();
            __register_frame(ptr);
            self.registrations.push(ptr as usize);
        } else {
            // For libunwind, `__register_frame` takes a pointer to a single FDE
            let start = eh_frame.as_ptr();
            let end = start.add(eh_frame.len());
            let mut current = start;

            // Walk all of the entries in the frame table and register them
            while current < end {
                let len = std::ptr::read::<u32>(current as *const u32) as usize;

                // Skip over the CIE and zero-length FDEs.
                // LLVM's libunwind emits a warning on zero-length FDEs.
                if current != start && len != 0 {
                    __register_frame(current);
                    self.registrations.push(current as usize);
                }

                // Move to the next table entry (+4 because the length itself is not inclusive)
                current = current.add(len + 4);
            }
        }
    }

    pub(crate) fn add_compact_unwind(
        &self,
        compact_unwind: Option<(wasmer_vm::SectionBodyPtr, usize)>,
    ) -> Result<(), String> {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        unsafe {
            if let Some((ptr, len)) = compact_unwind {
                _apple_uw::__unw_add_find_dynamic_unwind_sections(
                    _apple_uw::generate_find_dynamic_unwind_sections((*ptr) as usize, len),
                );
            }
        }

        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        {
            _ = compact_unwind;
        }
        Ok(())
    }
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
                for fde in self.registrations.iter().rev() {
                    __deregister_frame(*fde as *const _);
                }
            }
        }
    }
}
