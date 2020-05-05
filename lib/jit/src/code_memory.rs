//! Memory management for executable code.
use region;
use std::mem::ManuallyDrop;
use std::{cmp, mem};
use wasm_common::entity::PrimaryMap;
use wasm_common::LocalFunctionIndex;
use wasmer_compiler::FunctionBody;
use wasmer_runtime::{Mmap, VMFunctionBody};

struct CodeMemoryEntry {
    mmap: ManuallyDrop<Mmap>,
}

impl CodeMemoryEntry {
    fn new() -> Self {
        Self {
            mmap: ManuallyDrop::new(Mmap::new()),
        }
    }
    fn with_capacity(cap: usize) -> Result<Self, String> {
        Ok(Self {
            mmap: ManuallyDrop::new(Mmap::with_at_least(cap)?),
        })
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

        let (buf, start) = self.allocate(size)?;

        let (_, _, vmfunc) = Self::copy_function(func, start as u32, buf);

        Ok(vmfunc)
    }

    /// Allocate a continuous memory block for a compilation.
    ///
    /// Allocates memory for both the function bodies as well as function unwind data.
    pub fn allocate_for_compilation(
        &mut self,
        compilation: &PrimaryMap<LocalFunctionIndex, FunctionBody>,
    ) -> Result<Box<[&mut [VMFunctionBody]]>, String> {
        let total_len = compilation
            .values()
            .fold(0, |acc, func| acc + Self::function_allocation_size(func));

        let (mut buf, start) = self.allocate(total_len)?;
        let mut result = Vec::with_capacity(compilation.len());
        let mut start = start as u32;

        for func in compilation.values() {
            let (next_start, next_buf, vmfunc) = Self::copy_function(func, start, buf);

            result.push(vmfunc);

            start = next_start;
            buf = next_buf;
        }

        Ok(result.into_boxed_slice())
    }

    /// Make all allocated memory executable.
    pub fn publish(&mut self) {
        self.push_current(0)
            .expect("failed to push current memory map");

        for CodeMemoryEntry { mmap: m } in &mut self.entries[self.published..] {
            if !m.is_empty() {
                unsafe {
                    region::protect(m.as_mut_ptr(), m.len(), region::Protection::ReadExecute)
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
    ///
    /// TODO: Add an alignment flag.
    fn allocate(&mut self, size: usize) -> Result<(&mut [u8], usize), String> {
        if self.current.mmap.len() - self.position < size {
            self.push_current(cmp::max(0x10000, size))?;
        }

        let old_position = self.position;
        self.position += size;

        Ok((
            &mut self.current.mmap.as_mut_slice()[old_position..self.position],
            old_position,
        ))
    }

    /// Calculates the allocation size of the given compiled function.
    fn function_allocation_size(func: &FunctionBody) -> usize {
        if let Some(unwind_info) = &func.unwind_info {
            // Account for necessary unwind information alignment padding (32-bit)
            ((func.body.len() + 3) & !3) + unwind_info.len()
        } else {
            func.body.len()
        }
    }

    /// Copies the data of the compiled function to the given buffer.
    ///
    /// This will also add the function to the current function table.
    fn copy_function<'a>(
        func: &FunctionBody,
        func_start: u32,
        buf: &'a mut [u8],
    ) -> (u32, &'a mut [u8], &'a mut [VMFunctionBody]) {
        let func_end = func_start + (func.body.len() as u32);

        let (body, remainder) = buf.split_at_mut(func.body.len());
        body.copy_from_slice(&func.body);
        let vmfunc = Self::view_as_mut_vmfunc_slice(body);

        if func.unwind_info.is_none() {
            return (func_end, remainder, vmfunc);
        }
        let unwind_info = func.unwind_info.as_ref().unwrap();

        return (func_end, remainder, vmfunc);
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

#[cfg(test)]
mod tests {
    use super::CodeMemory;
    fn _assert() {
        fn _assert_send_sync<T: Send + Sync>() {}
        _assert_send_sync::<CodeMemory>();
    }
}
