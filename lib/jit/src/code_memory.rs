//! Memory management for executable code.
use crate::function_table::FunctionTable;
use region;
use std::mem::ManuallyDrop;
use std::{cmp, mem};
use wasm_common::entity::PrimaryMap;
use wasm_common::LocalFuncIndex;
use wasmer_compiler::{Compilation, CompiledFunction};
use wasmer_runtime::{Mmap, VMFunctionBody};

struct CodeMemoryEntry {
    mmap: ManuallyDrop<Mmap>,
    table: ManuallyDrop<FunctionTable>,
}

impl CodeMemoryEntry {
    fn new() -> Self {
        Self {
            mmap: ManuallyDrop::new(Mmap::new()),
            table: ManuallyDrop::new(FunctionTable::new()),
        }
    }
    fn with_capacity(cap: usize) -> Result<Self, String> {
        Ok(Self {
            mmap: ManuallyDrop::new(Mmap::with_at_least(cap)?),
            table: ManuallyDrop::new(FunctionTable::new()),
        })
    }
}

impl Drop for CodeMemoryEntry {
    fn drop(&mut self) {
        unsafe {
            // Table needs to be freed before mmap.
            ManuallyDrop::drop(&mut self.table);
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
        compilation: &Compilation,
    ) -> Result<PrimaryMap<LocalFuncIndex, *mut [VMFunctionBody]>, String> {
        let fat_ptrs = self.allocate_for_compilation(compilation)?;

        // Second, create a PrimaryMap from result vector of pointers.
        let mut result = PrimaryMap::with_capacity(compilation.len());
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
        func: &CompiledFunction,
    ) -> Result<&mut [VMFunctionBody], String> {
        let size = Self::function_allocation_size(func);

        let (buf, table, start) = self.allocate(size)?;

        let (_, _, _, vmfunc) = Self::copy_function(func, start as u32, buf, table);

        Ok(vmfunc)
    }

    /// Allocate a continuous memory block for a compilation.
    ///
    /// Allocates memory for both the function bodies as well as function unwind data.
    pub fn allocate_for_compilation(
        &mut self,
        compilation: &Compilation,
    ) -> Result<Box<[&mut [VMFunctionBody]]>, String> {
        let total_len = compilation
            .into_iter()
            .fold(0, |acc, func| acc + Self::function_allocation_size(func));

        let (mut buf, mut table, start) = self.allocate(total_len)?;
        let mut result = Vec::with_capacity(compilation.len());
        let mut start = start as u32;

        for func in compilation.into_iter() {
            let (next_start, next_buf, next_table, vmfunc) =
                Self::copy_function(func, start, buf, table);

            result.push(vmfunc);

            start = next_start;
            buf = next_buf;
            table = next_table;
        }

        Ok(result.into_boxed_slice())
    }

    /// Make all allocated memory executable.
    pub fn publish(&mut self) {
        self.push_current(0)
            .expect("failed to push current memory map");

        for CodeMemoryEntry { mmap: m, table: t } in &mut self.entries[self.published..] {
            // Remove write access to the pages due to the relocation fixups.
            t.publish(m.as_ptr() as u64)
                .expect("failed to publish function table");

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
    fn allocate(&mut self, size: usize) -> Result<(&mut [u8], &mut FunctionTable, usize), String> {
        if self.current.mmap.len() - self.position < size {
            self.push_current(cmp::max(0x10000, size))?;
        }

        let old_position = self.position;
        self.position += size;

        Ok((
            &mut self.current.mmap.as_mut_slice()[old_position..self.position],
            &mut self.current.table,
            old_position,
        ))
    }

    /// Calculates the allocation size of the given compiled function.
    fn function_allocation_size(func: &CompiledFunction) -> usize {
        if func.unwind_info.is_empty() {
            func.body.len()
        } else {
            // Account for necessary unwind information alignment padding (32-bit)
            ((func.body.len() + 3) & !3) + func.unwind_info.len()
        }
    }

    /// Copies the data of the compiled function to the given buffer.
    ///
    /// This will also add the function to the current function table.
    fn copy_function<'a>(
        func: &CompiledFunction,
        func_start: u32,
        buf: &'a mut [u8],
        table: &'a mut FunctionTable,
    ) -> (
        u32,
        &'a mut [u8],
        &'a mut FunctionTable,
        &'a mut [VMFunctionBody],
    ) {
        let func_end = func_start + (func.body.len() as u32);

        let (body, remainder) = buf.split_at_mut(func.body.len());
        body.copy_from_slice(&func.body);
        let vmfunc = Self::view_as_mut_vmfunc_slice(body);

        if func.unwind_info.is_empty() {
            return (func_end, remainder, table, vmfunc);
        }

        // Keep unwind information 32-bit aligned (round up to the nearest 4 byte boundary)
        let padding = ((func.body.len() + 3) & !3) - func.body.len();
        let (unwind, remainder) = remainder.split_at_mut(padding + func.unwind_info.len());
        let mut relocs = Vec::new();
        func.unwind_info
            .serialize(&mut unwind[padding..], &mut relocs);

        let unwind_start = func_end + (padding as u32);
        let unwind_end = unwind_start + (func.unwind_info.len() as u32);

        relocs.iter_mut().for_each(move |r| {
            r.offset += unwind_start;
            r.addend += func_start as i64;
        });

        table.add_function(func_start, func_end, unwind_start, &relocs);

        (unwind_end, remainder, table, vmfunc)
    }

    /// Convert mut a slice from u8 to VMFunctionBody.
    fn view_as_mut_vmfunc_slice(slice: &mut [u8]) -> &mut [VMFunctionBody] {
        let byte_ptr: *mut [u8] = slice;
        let body_ptr = byte_ptr as *mut [VMFunctionBody];
        unsafe { &mut *body_ptr }
    }

    /// Pushes the current Mmap (and function table) and allocates a new Mmap of the given size.
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
        } else {
            assert_eq!(previous.table.len(), 0);
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
