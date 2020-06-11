//! Module for System V ABI unwind registry.
use wasmer_compiler::CompiledFunctionUnwindInfo;

/// Represents a registry of function unwind information for System V ABI.
pub struct UnwindRegistry {
    base_address: usize,
    functions: Vec<gimli::write::FrameDescriptionEntry>,
    frame_table: Vec<u8>,
    registrations: Vec<usize>,
    published: bool,
}

extern "C" {
    // libunwind import
    fn __register_frame(fde: *const u8);
    fn __deregister_frame(fde: *const u8);
}

impl UnwindRegistry {
    /// Creates a new unwind registry with the given base address.
    pub fn new(base_address: usize) -> Self {
        Self {
            base_address,
            functions: Vec::new(),
            frame_table: Vec::new(),
            registrations: Vec::new(),
            published: false,
        }
    }

    /// Registers a function given the start offset, length, and unwind information.
    pub fn register(&mut self, _func_start: u32, _func_len: u32, _info: &CompiledFunctionUnwindInfo) -> Result<(), String> {
        // Do nothing
        Ok(())
    }

    /// Publishes all registered functions.
    pub fn publish(&mut self, isa: &dyn TargetIsa) -> Result<()> {
        if self.published {
            bail!("unwind registry has already been published");
        }

        if self.functions.is_empty() {
            self.published = true;
            return Ok(());
        }

        unsafe {
            self.register_frames();
        }

        self.published = true;

        Ok(())
    }


    unsafe fn register_frames(&mut self) {
        cfg_if::cfg_if! {
            if #[cfg(target_os = "macos")] {
                // On macOS, `__register_frame` takes a pointer to a single FDE
                let start = self.frame_table.as_ptr();
                let end = start.add(self.frame_table.len());
                let mut current = start;

                // Walk all of the entries in the frame table and register them
                while current < end {
                    let len = std::ptr::read::<u32>(current as *const u32) as usize;

                    // Skip over the CIE
                    if current != start {
                        __register_frame(current);
                        self.registrations.push(current as usize);
                    }

                    // Move to the next table entry (+4 because the length itself is not inclusive)
                    current = current.add(len + 4);
                }
            } else {
                // On other platforms, `__register_frame` will walk the FDEs until an entry of length 0
                let ptr = self.frame_table.as_ptr();
                __register_frame(ptr);
                self.registrations.push(ptr as usize);
            }
        }
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
