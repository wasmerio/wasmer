use crate::error::GrowError;
use crate::{error::CreationError, sys, types::MemoryType, units::Pages, vm};

#[doc(hidden)]
pub const SAFE_STATIC_HEAP_SIZE: usize = 1 << 32; // 4 GiB
#[doc(hidden)]
pub const SAFE_STATIC_GUARD_SIZE: usize = 1 << 31; // 2 GiB

/// This is an internal-only api.
///
/// A static memory allocates 6GB of *virtual* memory when created
/// in order to allow the WebAssembly module to contain no bounds-checks.
///
/// Additionally, static memories stay at a single virtual address, so there is no need
/// to reload its address on each use.
///
/// Static memories take a relatively long time to create, so if memories are short-lived,
/// it's recommended that a dynamic memory is used. There is currently no user-facing api that
/// allows them to select the type of memory used however.
pub struct StaticMemory {
    memory: sys::Memory,
    current: Pages,
    max: Option<Pages>,
}

impl StaticMemory {
    pub(in crate::memory) fn new(
        desc: MemoryType,
        local: &mut vm::LocalMemory,
    ) -> Result<Box<Self>, CreationError> {
        let memory = {
            let mut memory = sys::Memory::with_size(SAFE_STATIC_HEAP_SIZE + SAFE_STATIC_GUARD_SIZE)
                .map_err(|_| CreationError::UnableToCreateMemory)?;
            if desc.minimum != Pages(0) {
                unsafe {
                    memory
                        .protect(0..desc.minimum.bytes().0, sys::Protect::ReadWrite)
                        .map_err(|_| CreationError::UnableToCreateMemory)?;
                }
            }

            memory
        };

        let mut storage = Box::new(StaticMemory {
            memory,
            current: desc.minimum,
            max: desc.maximum,
        });
        let storage_ptr: *mut StaticMemory = &mut *storage;

        local.base = storage.memory.as_ptr();
        local.bound = desc.minimum.bytes().0;
        local.memory = storage_ptr as *mut ();

        Ok(storage)
    }

    /// The size of this memory in `Pages`.
    pub fn size(&self) -> Pages {
        self.current
    }

    /// Try to grow this memory by the given number of delta pages.
    pub fn grow(&mut self, delta: Pages, local: &mut vm::LocalMemory) -> Result<Pages, GrowError> {
        if delta == Pages(0) {
            return Ok(self.current);
        }

        let new_pages = self.current.checked_add(delta).map_err(|e| e.into())?;

        if let Some(max) = self.max {
            if new_pages > max {
                return Err(GrowError::ExceededMaxPagesForMemory(
                    new_pages.0 as usize,
                    max.0 as usize,
                ));
            }
        }

        let _ = unsafe {
            self.memory
                .protect(
                    self.current.bytes().0..new_pages.bytes().0,
                    sys::Protect::ReadWrite,
                )
                .map_err(|e| e.into())
        }?;

        local.bound = new_pages.bytes().0;

        let old_pages = self.current;

        self.current = new_pages;

        Ok(old_pages)
    }

    /// Get this memory represented as a slice of bytes.
    pub fn as_slice(&self) -> &[u8] {
        unsafe { &self.memory.as_slice()[0..self.current.bytes().0] }
    }

    /// Get this memory represented as a mutable slice of bytes.
    pub fn as_slice_mut(&mut self) -> &mut [u8] {
        unsafe { &mut self.memory.as_slice_mut()[0..self.current.bytes().0] }
    }
}
