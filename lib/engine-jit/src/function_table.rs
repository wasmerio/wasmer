//! Runtime function table.
//!
//! This module is primarily used to track JIT functions on Windows for stack walking and unwind.
use wasmer_compiler::FunctionTableReloc;

/// Represents a runtime function table.
///
/// This is used to register JIT code with the operating system to enable stack walking and unwinding.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
pub struct FunctionTable {
    functions: Vec<winapi::um::winnt::RUNTIME_FUNCTION>,
    published: bool,
}

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
impl FunctionTable {
    /// Creates a new function table.
    pub fn new() -> Self {
        Self {
            functions: Vec::new(),
            published: false,
        }
    }

    /// Returns the number of functions in the table, also referred to as its 'length'.
    pub fn len(&self) -> usize {
        self.functions.len()
    }

    /// Returns whether or not the function table is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Adds a function to the table based off of the start offset, end offset, and unwind offset.
    ///
    /// The offsets are from the "module base", which is provided when the table is published.
    pub fn add_function(
        &mut self,
        start: u32,
        end: u32,
        unwind: u32,
        _relocs: &[FunctionTableReloc],
    ) {
        assert_eq!(_relocs.len(), 0);
        use winapi::um::winnt;

        assert!(!self.published, "table has already been published");

        let mut entry = winnt::RUNTIME_FUNCTION::default();

        entry.BeginAddress = start;
        entry.EndAddress = end;

        unsafe {
            *entry.u.UnwindInfoAddress_mut() = unwind;
        }

        self.functions.push(entry);
    }

    /// Publishes the function table using the given base address.
    ///
    /// A published function table will automatically be deleted when it is dropped.
    pub fn publish(&mut self, base_address: u64) -> Result<(), String> {
        use winapi::um::winnt;

        if self.published {
            return Err("function table was already published".into());
        }

        self.published = true;

        if self.functions.is_empty() {
            return Ok(());
        }

        unsafe {
            // Windows heap allocations are 32-bit aligned, but assert just in case
            assert_eq!(
                (self.functions.as_mut_ptr() as u64) % 4,
                0,
                "function table allocation was not aligned"
            );

            if winnt::RtlAddFunctionTable(
                self.functions.as_mut_ptr(),
                self.functions.len() as u32,
                base_address,
            ) == 0
            {
                return Err("failed to add function table".into());
            }
        }

        Ok(())
    }
}

#[cfg(target_os = "windows")]
impl Drop for FunctionTable {
    fn drop(&mut self) {
        use winapi::um::winnt;

        if self.published {
            unsafe {
                winnt::RtlDeleteFunctionTable(self.functions.as_mut_ptr());
            }
        }
    }
}

/// Represents a runtime function table.
///
/// This is used to register JIT code with the operating system to enable stack walking and unwinding.
#[cfg(unix)]
pub struct FunctionTable {
    functions: Vec<u32>,
    relocs: Vec<FunctionTableReloc>,
    published: Option<Vec<usize>>,
}

#[cfg(unix)]
impl FunctionTable {
    /// Creates a new function table.
    pub fn new() -> Self {
        Self {
            functions: Vec::new(),
            relocs: Vec::new(),
            published: None,
        }
    }

    /// Returns the number of functions in the table, also referred to as its 'length'.
    pub fn len(&self) -> usize {
        self.functions.len()
    }

    /// Returns whether or not the function table is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Adds a function to the table based off of the start offset, end offset, and unwind offset.
    ///
    /// The offsets are from the "module base", which is provided when the table is published.
    pub fn add_function(
        &mut self,
        _start: u32,
        _end: u32,
        unwind: u32,
        relocs: &[FunctionTableReloc],
    ) {
        assert!(self.published.is_none(), "table has already been published");
        self.functions.push(unwind);
        self.relocs.extend_from_slice(relocs);
    }

    /// Publishes the function table using the given base address.
    ///
    /// A published function table will automatically be deleted when it is dropped.
    pub fn publish(&mut self, base_address: u64) -> Result<(), String> {
        if self.published.is_some() {
            return Err("function table was already published".into());
        }

        if self.functions.is_empty() {
            assert_eq!(self.relocs.len(), 0);
            self.published = Some(vec![]);
            return Ok(());
        }

        extern "C" {
            // libunwind import
            fn __register_frame(fde: *const u8);
        }

        for reloc in self.relocs.iter() {
            let addr = base_address + (reloc.offset as u64);
            let target = base_address + (reloc.addend as u64);
            unsafe {
                std::ptr::write(addr as *mut u64, target);
            }
        }

        let mut fdes = Vec::with_capacity(self.functions.len());
        for unwind_offset in self.functions.iter() {
            let addr = base_address + (*unwind_offset as u64);
            let off = unsafe { std::ptr::read::<u32>(addr as *const u32) } as usize + 4;

            let fde = (addr + off as u64) as usize;
            unsafe {
                __register_frame(fde as *const _);
            }
            fdes.push(fde);
        }

        self.published = Some(fdes);
        Ok(())
    }
}

#[cfg(unix)]
impl Drop for FunctionTable {
    fn drop(&mut self) {
        extern "C" {
            // libunwind import
            fn __deregister_frame(fde: *const u8);
        }

        if let Some(published) = &self.published {
            unsafe {
                // I'm not really sure why, but it appears to be way faster to
                // unregister frames in reverse order rather than in-order. This
                // way we're deregistering in LIFO order, and maybe there's some
                // vec shifting or something like that in libgcc?
                //
                // Locally on Ubuntu 18.04 a wasm module with 40k empty
                // functions takes 0.1s to compile and drop with reverse
                // iteration. With forward iteration it takes 3s to compile and
                // drop!
                //
                // Poking around libgcc sources seems to indicate that some sort
                // of linked list is being traversed... We may need to figure
                // out something else for backtraces in the future since this
                // API may not be long-lived to keep calling.
                for fde in published.iter().rev() {
                    __deregister_frame(*fde as *const _);
                }
            }
        }
    }
}
