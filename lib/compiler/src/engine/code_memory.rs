// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

//! Memory management for executable code.
use super::unwind::UnwindRegistry;
use crate::{
    GlobalFrameInfoRegistration,
    types::{
        function::FunctionBodyLike,
        unwind::{CompiledFunctionUnwindInfoLike, CompiledFunctionUnwindInfoReference},
    },
};
use wasmer_vm::{Mmap, VMFunctionBody};

/// The optimal alignment for functions.
///
/// On x86-64, this is 16 since it's what the optimizations assume.
/// When we add support for other architectures, we should also figure out their
/// optimal alignment values.
const ARCH_FUNCTION_ALIGNMENT: usize = 16;

/// The optimal alignment for data.
///
const DATA_SECTION_ALIGNMENT: usize = 64;

/// Memory manager for executable code.
pub struct CodeMemory {
    // frame info is placed first, to ensure it's dropped before the mmap
    unwind_registry: UnwindRegistry,
    mmap: Mmap,
    start_of_nonexecutable_pages: usize,
}

impl CodeMemory {
    /// Create a new `CodeMemory` instance.
    pub fn new() -> Self {
        Self {
            unwind_registry: UnwindRegistry::new(),
            mmap: Mmap::new(),
            start_of_nonexecutable_pages: 0,
        }
    }

    /// Mutably get the UnwindRegistry.
    pub fn unwind_registry_mut(&mut self) -> &mut UnwindRegistry {
        &mut self.unwind_registry
    }

    /// Apply the page permissions.
    pub fn publish(&mut self) {
        if self.mmap.is_empty() || self.start_of_nonexecutable_pages == 0 {
            return;
        }
        assert!(self.mmap.len() >= self.start_of_nonexecutable_pages);
        unsafe {
            region::protect(
                self.mmap.as_mut_ptr(),
                self.start_of_nonexecutable_pages,
                region::Protection::READ_EXECUTE,
            )
        }
        .expect("unable to make memory readonly and executable");
    }

    /// Calculates the allocation size of the given compiled function.
    fn function_allocation_size<'a>(func: &'a impl FunctionBodyLike<'a>) -> usize {
        match &func.unwind_info().map(|o| o.get()) {
            Some(CompiledFunctionUnwindInfoReference::WindowsX64(info)) => {
                // Windows unwind information is required to be emitted into code memory
                // This is because it must be a positive relative offset from the start of the memory
                // Account for necessary unwind information alignment padding (32-bit alignment)
                func.body().len().next_multiple_of(4) + info.len()
            }
            _ => func.body().len(),
        }
    }

    /// Copies the data of the compiled function to the given buffer.
    ///
    /// This will also add the function to the current function table.
    fn copy_function<'module, 'memory>(
        registry: &mut UnwindRegistry,
        func: &'module impl FunctionBodyLike<'module>,
        buf: &'memory mut [u8],
    ) -> &'memory mut [VMFunctionBody] {
        assert!((buf.as_ptr() as usize).is_multiple_of(ARCH_FUNCTION_ALIGNMENT));

        let func_len = func.body().len();

        let (body, remainder) = buf.split_at_mut(func_len);
        body.copy_from_slice(func.body());
        let vmfunc = Self::view_as_mut_vmfunc_slice(body);

        let unwind_info = func.unwind_info().map(|o| o.get());
        if let Some(CompiledFunctionUnwindInfoReference::WindowsX64(info)) = unwind_info {
            // Windows unwind information is written following the function body
            // Keep unwind information 32-bit aligned (round up to the nearest 4 byte boundary)
            let unwind_start = func_len.next_multiple_of(4);
            let unwind_size = info.len();
            let padding = unwind_start - func_len;
            assert!((func_len + padding).is_multiple_of(4));
            let slice = remainder.split_at_mut(padding + unwind_size).0;
            slice[padding..].copy_from_slice(info);
        }

        if let Some(ref info) = unwind_info {
            registry
                .register(vmfunc.as_ptr() as usize, 0, func_len as u32, info)
                .expect("failed to register unwind information");
        }

        vmfunc
    }

    /// Convert mut a slice from u8 to VMFunctionBody.
    fn view_as_mut_vmfunc_slice(slice: &mut [u8]) -> &mut [VMFunctionBody] {
        let byte_ptr: *mut [u8] = slice;
        let body_ptr = byte_ptr as *mut [VMFunctionBody];
        unsafe { &mut *body_ptr }
    }
}

fn round_up(size: usize, multiple: usize) -> usize {
    debug_assert!(multiple.is_power_of_two());
    size.next_multiple_of(multiple)
}

#[cfg(test)]
mod tests {
    use super::CodeMemory;
    fn _assert() {
        fn _assert_send_sync<T: Send + Sync>() {}
        _assert_send_sync::<CodeMemory>();
    }
}
