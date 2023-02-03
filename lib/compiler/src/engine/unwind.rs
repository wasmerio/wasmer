// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

#[cfg(all(windows, target_arch = "x86_64"))]
use std::collections::HashMap;

use wasmer_types::CompiledFunctionUnwindInfo;

#[cfg(all(windows, target_arch = "x86_64"))]
use winapi::um::winnt;

/// Unwind will work for Dwarf or Windows64 unwind infoo
/// And will fall back for a dummy implementation for other types

#[derive(PartialEq, Debug)]
enum UnwindType {
    Unknown,
    Dummy,
    #[cfg(unix)]
    SystemV,
    #[cfg(all(windows, target_arch = "x86_64"))]
    WindowsX64,
}

/// Represents a registry of function unwind information for System V or Windows X64 ABI.
/// Cross-compiling will not handle the Unwind info for now,
/// and will fallback to the Dummy implementation
pub struct UnwindRegistry {
    ty: UnwindType,
    // WINNT: A hashmap mapping the baseaddress with the registered runtime functions
    #[cfg(all(windows, target_arch = "x86_64"))]
    functions: HashMap<usize, Vec<winnt::RUNTIME_FUNCTION>>,
    // SYSV: registraction vector
    #[cfg(unix)]
    registrations: Vec<usize>,
    // common: published?
    published: bool,
}

// SystemV helper
#[cfg(unix)]
extern "C" {
    // libunwind import
    fn __register_frame(fde: *const u8);
    fn __deregister_frame(fde: *const u8);
}

impl UnwindRegistry {
    /// Creates a new unwind registry with the given base address.
    pub fn new() -> Self {
        Self {
            ty: UnwindType::Unknown,
            #[cfg(all(windows, target_arch = "x86_64"))]
            functions: HashMap::new(),
            #[cfg(unix)]
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
        info: &CompiledFunctionUnwindInfo,
    ) -> Result<(), String> {
        if self.published {
            return Err("unwind registry has already been published".to_string());
        }

        match info {
            // Windows Unwind need to use Windows system function for now
            // No unwind information will be handled only on the Windows platform itself
            // Cross-compiling will fallback to the Dummy implementation
            #[cfg(all(windows, target_arch = "x86_64"))]
            CompiledFunctionUnwindInfo::WindowsX64(_) => {
                if self.ty != UnwindType::Unknown && self.ty != UnwindType::WindowsX64 {
                    return Err("unwind registry has already un incompatible type".to_string());
                }
                self.ty = UnwindType::WindowsX64;
                let mut entry = winnt::RUNTIME_FUNCTION::default();

                entry.BeginAddress = _func_start;
                entry.EndAddress = _func_start + _func_len;

                // The unwind information should be immediately following the function
                // with padding for 4 byte alignment
                unsafe {
                    *entry.u.UnwindInfoAddress_mut() = (entry.EndAddress + 3) & !3;
                }
                let entries = self
                    .functions
                    .entry(_base_address)
                    .or_insert_with(|| Vec::new());

                entries.push(entry);
            }
            #[cfg(unix)]
            CompiledFunctionUnwindInfo::Dwarf => {
                if self.ty != UnwindType::Unknown && self.ty != UnwindType::SystemV {
                    return Err("unwind registry has already un incompatible type".to_string());
                }
                self.ty = UnwindType::SystemV;
            }
            _ => {
                if self.ty != UnwindType::Unknown && self.ty != UnwindType::Dummy {
                    return Err("unwind registry has already un incompatible type".to_string());
                }
                self.ty = UnwindType::Dummy;
            }
        };
        Ok(())
    }

    /// Publishes all registered functions.
    pub fn publish(&mut self, eh_frame: Option<&[u8]>) -> Result<(), String> {
        if self.published {
            return Err("unwind registry has already been published".to_string());
        }

        let have_eh_frame = eh_frame.is_some();
        match self.ty {
            #[cfg(unix)]
            UnwindType::SystemV => {}
            #[cfg(all(windows, target_arch = "x86_64"))]
            UnwindType::WindowsX64 => {
                if have_eh_frame {
                    return Err("unwind mysmatch eh_frame on WindowsX64".to_string());
                }
            }
            UnwindType::Dummy => {}
            UnwindType::Unknown =>
            {
                #[cfg(unix)]
                if have_eh_frame {
                    self.ty = UnwindType::SystemV;
                }
            }
        }

        match self.ty {
            #[cfg(unix)]
            UnwindType::SystemV => {
                if let Some(eh_frame) = eh_frame {
                    unsafe {
                        self.register_frames(eh_frame);
                    }
                }
            }
            #[cfg(all(windows, target_arch = "x86_64"))]
            UnwindType::WindowsX64 => {
                if !self.functions.is_empty() {
                    for (_base_address, functions) in self.functions.iter_mut() {
                        // Windows heap allocations are 32-bit aligned, but assert just in case
                        assert_eq!(
                            (functions.as_mut_ptr() as u64) % 4,
                            0,
                            "function table allocation was not aligned"
                        );
                        unsafe {
                            if winnt::RtlAddFunctionTable(
                                functions.as_mut_ptr(),
                                functions.len() as u32,
                                *_base_address as u64,
                            ) == 0
                            {
                                return Err("failed to register function tables".to_string());
                            }
                        }
                    }
                }
            }
            _ => {}
        };

        self.published = true;
        Ok(())
    }

    #[allow(clippy::cast_ptr_alignment)]
    #[cfg(unix)]
    unsafe fn register_frames(&mut self, eh_frame: &[u8]) {
        if cfg!(any(
            all(target_os = "linux", target_env = "gnu"),
            target_os = "freebsd"
        )) {
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
}

impl Drop for UnwindRegistry {
    fn drop(&mut self) {
        if self.published {
            match self.ty {
                #[cfg(all(windows, target_arch = "x86_64"))]
                UnwindType::WindowsX64 => unsafe {
                    for functions in self.functions.values_mut() {
                        winnt::RtlDeleteFunctionTable(functions.as_mut_ptr());
                    }
                },
                #[cfg(unix)]
                UnwindType::SystemV => {
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
                _ => {}
            }
        }
    }
}
