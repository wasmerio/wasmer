//! Memory management for executable code.
use crate::unwind::UnwindRegistry;
use region;
use std::mem::ManuallyDrop;
use std::ptr;
use std::{cmp, mem};
use wasm_common::entity::PrimaryMap;
use wasm_common::LocalFunctionIndex;
use wasmer_compiler::{CompiledFunctionUnwindInfo, FunctionBody, SectionBody};
use wasmer_runtime::{Mmap, VMFunctionBody};

/// The optimal alignment for functions.
///
/// On x86-64, this is 16 since it's what the optimizations assume.
/// When we add support for other architectures, we should also figure out their
/// optimal alignment values.
const ARCH_FUNCTION_ALIGNMENT: usize = 16;

struct CodeMemoryEntry {
    mmap: ManuallyDrop<Mmap>,
    registry: ManuallyDrop<UnwindRegistry>,
}

impl CodeMemoryEntry {
    fn new() -> Self {
        let mmap = ManuallyDrop::new(Mmap::new());
        let registry = ManuallyDrop::new(UnwindRegistry::new(mmap.as_ptr() as usize));
        Self { mmap, registry }
    }
    fn with_capacity(cap: usize) -> Result<Self, String> {
        let mmap = ManuallyDrop::new(Mmap::with_at_least(cap)?);
        let registry = ManuallyDrop::new(UnwindRegistry::new(mmap.as_ptr() as usize));
        Ok(Self { mmap, registry })
    }
}

impl Drop for CodeMemoryEntry {
    fn drop(&mut self) {
        unsafe {
            // Table needs to be freed before mmap.
            ManuallyDrop::drop(&mut self.mmap);
        }
    }
}

/// Memory manager for executable code.
pub struct CodeMemory {
    current: CodeMemoryEntry,
    entries: Vec<CodeMemoryEntry>,
    position: usize,
    published: usize,
}

impl CodeMemory {
    /// Create a new `CodeMemory` instance.
    pub fn new() -> Self {
        Self {
            current: CodeMemoryEntry::new(),
            entries: Vec::new(),
            position: 0,
            published: 0,
        }
    }

    /// Allocate a continuous memory blocks for a single compiled function.
    pub fn allocate_functions(
        &mut self,
        functions: &PrimaryMap<LocalFunctionIndex, FunctionBody>,
    ) -> Result<PrimaryMap<LocalFunctionIndex, *mut [VMFunctionBody]>, String> {
        let fat_ptrs = self.allocate_for_compilation(functions)?;

        // Second, create a PrimaryMap from result vector of pointers.
        let mut result = PrimaryMap::with_capacity(functions.len());
        for i in 0..fat_ptrs.len() {
            let fat_ptr: *mut [VMFunctionBody] = fat_ptrs[i];
            result.push(fat_ptr);
        }
        Ok(result)
    }

    /// Allocate a continuous memory block for a single compiled function.
    /// TODO: Reorganize the code that calls this to emit code directly into the
    /// mmap region rather than into a Vec that we need to copy in.
    pub fn allocate_for_function(
        &mut self,
        func: &FunctionBody,
    ) -> Result<&mut [VMFunctionBody], String> {
        let size = Self::function_allocation_size(func);

        let (buf, registry, start) = self.allocate(size, ARCH_FUNCTION_ALIGNMENT)?;

        let (_, _, vmfunc) = Self::copy_function(func, start as u32, buf, registry);
        assert!(vmfunc as *mut _ as *mut u8 as usize % ARCH_FUNCTION_ALIGNMENT == 0);

        Ok(vmfunc)
    }

    /// Allocate a continuous memory block for an executable custom section.
    pub fn allocate_for_executable_custom_section(
        &mut self,
        section: &SectionBody,
    ) -> Result<&mut [u8], String> {
        let section = section.as_slice();
        let (buf, _, _) = self.allocate(section.len(), ARCH_FUNCTION_ALIGNMENT)?;
        buf.copy_from_slice(section);
        Ok(buf)
    }

    /// Allocate a continuous memory block for a compilation.
    ///
    /// Allocates memory for both the function bodies as well as function unwind data.
    pub fn allocate_for_compilation(
        &mut self,
        compilation: &PrimaryMap<LocalFunctionIndex, FunctionBody>,
    ) -> Result<Box<[&mut [VMFunctionBody]]>, String> {
        let total_len = compilation.values().fold(0, |acc, func| {
            acc + get_align_padding_size(acc, ARCH_FUNCTION_ALIGNMENT)
                + Self::function_allocation_size(func)
        });

        let (mut buf, registry, start) = self.allocate(total_len, ARCH_FUNCTION_ALIGNMENT)?;
        let mut result = Vec::with_capacity(compilation.len());
        let mut start = start as u32;
        let mut padding = 0usize;

        for func in compilation.values() {
            let (next_start, next_buf, vmfunc) =
                Self::copy_function(func, start + padding as u32, &mut buf[padding..], registry);
            assert!(vmfunc as *mut _ as *mut u8 as usize % ARCH_FUNCTION_ALIGNMENT == 0);

            result.push(vmfunc);

            padding = get_align_padding_size(next_start as usize, ARCH_FUNCTION_ALIGNMENT);
            start = next_start;
            buf = next_buf;
        }

        Ok(result.into_boxed_slice())
    }

    /// Make all allocated memory executable.
    pub fn publish(&mut self) {
        self.push_current(0)
            .expect("failed to push current memory map");

        for CodeMemoryEntry { mmap: m, .. } in &mut self.entries[self.published..] {
            if !m.is_empty() {
                unsafe {
                    region::protect(m.as_mut_ptr(), m.len(), region::Protection::READ_EXECUTE)
                }
                .expect("unable to make memory readonly and executable");
            }
        }

        self.published = self.entries.len();
    }

    /// Allocate `size` bytes of memory which can be made executable later by
    /// calling `publish()`. Note that we allocate the memory as writeable so
    /// that it can be written to and patched, though we make it readonly before
    /// actually executing from it.
    ///
    /// A few values are returned:
    ///
    /// * A mutable slice which references the allocated memory
    /// * A function table instance where unwind information is registered
    /// * The offset within the current mmap that the slice starts at
    fn allocate(
        &mut self,
        size: usize,
        alignment: usize,
    ) -> Result<(&mut [u8], &mut UnwindRegistry, usize), String> {
        assert!(alignment > 0);

        let align_padding = get_align_padding_size(self.position, alignment);
        let padded_size = size + align_padding;

        let old_position;

        if self.current.mmap.len() - self.position < padded_size {
            // If we are allocating a new region, then it is already aligned to page boundary - no need to apply padding here.
            self.push_current(cmp::max(0x10000, size))?;
            old_position = 0;
            self.position += size;
        } else {
            // Otherwise, apply padding.
            old_position = self.position + align_padding;
            self.position += padded_size;
        }

        assert!(old_position % alignment == 0);

        Ok((
            &mut self.current.mmap.as_mut_slice()[old_position..self.position],
            &mut self.current.registry,
            old_position,
        ))
    }

    /// Calculates the allocation size of the given compiled function.
    fn function_allocation_size(func: &FunctionBody) -> usize {
        match &func.unwind_info {
            Some(CompiledFunctionUnwindInfo::WindowsX64(info)) => {
                // Windows unwind information is required to be emitted into code memory
                // This is because it must be a positive relative offset from the start of the memory
                // Account for necessary unwind information alignment padding (32-bit alignment)
                ((func.body.len() + 3) & !3) + info.len()
            }
            _ => func.body.len(),
        }
    }

    /// Copies the data of the compiled function to the given buffer.
    ///
    /// This will also add the function to the current function table.
    fn copy_function<'a>(
        func: &FunctionBody,
        func_start: u32,
        buf: &'a mut [u8],
        registry: &mut UnwindRegistry,
    ) -> (u32, &'a mut [u8], &'a mut [VMFunctionBody]) {
        let func_len = func.body.len();
        let mut func_end = func_start + (func_len as u32);

        let (body, mut remainder) = buf.split_at_mut(func_len);
        body.copy_from_slice(&func.body);
        let vmfunc = Self::view_as_mut_vmfunc_slice(body);

        if let Some(CompiledFunctionUnwindInfo::WindowsX64(info)) = &func.unwind_info {
            // Windows unwind information is written following the function body
            // Keep unwind information 32-bit aligned (round up to the nearest 4 byte boundary)
            let unwind_start = (func_end + 3) & !3;
            let unwind_size = info.len();
            let padding = (unwind_start - func_end) as usize;

            let (slice, r) = remainder.split_at_mut(padding + unwind_size);
            slice[padding..].copy_from_slice(&info);

            func_end = unwind_start + (unwind_size as u32);
            remainder = r;
        }

        if let Some(info) = &func.unwind_info {
            registry
                .register(func_start, func_len as u32, info)
                .expect("failed to register unwind information");
        }

        (func_end, remainder, vmfunc)
    }

    /// Convert mut a slice from u8 to VMFunctionBody.
    fn view_as_mut_vmfunc_slice(slice: &mut [u8]) -> &mut [VMFunctionBody] {
        let byte_ptr: *mut [u8] = slice;
        let body_ptr = byte_ptr as *mut [VMFunctionBody];
        unsafe { &mut *body_ptr }
    }

    /// Pushes the current Mmap and allocates a new Mmap of the given size.
    fn push_current(&mut self, new_size: usize) -> Result<(), String> {
        let previous = mem::replace(
            &mut self.current,
            if new_size == 0 {
                CodeMemoryEntry::new()
            } else {
                CodeMemoryEntry::with_capacity(cmp::max(0x10000, new_size))?
            },
        );

        if !previous.mmap.is_empty() {
            self.entries.push(previous);
        }

        self.position = 0;

        Ok(())
    }
}

/// Calculates the minimum number of padding bytes required to fulfill `alignment`.
fn get_align_padding_size(position: usize, alignment: usize) -> usize {
    match position % alignment {
        0 => 0,
        x => alignment - x,
    }
}

#[cfg(test)]
mod tests {
    use super::CodeMemory;
    fn _assert() {
        fn _assert_send_sync<T: Send + Sync>() {}
        _assert_send_sync::<CodeMemory>();
    }
}
