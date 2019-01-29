use crate::{
    memory::{
        static_::{SAFE_STATIC_GUARD_SIZE, SAFE_STATIC_HEAP_SIZE},
        WASM_MAX_PAGES, WASM_PAGE_SIZE,
    },
    sys,
    types::MemoryDesc,
    vm,
};

/// This is an internal-only api.
///
/// A static memory allocates 6GB of *virtual* memory when created
/// in order to allow the webassembly module to contain no bounds-checks.
///
/// Additionally, static memories stay at a single virtual address, so there is no need
/// to reload its address on each use.
///
/// Static memories take a relatively long time to create, so if memories are short-lived,
/// it's recommended that a dynamic memory is used. There is currently no user-facing api that
/// allows them to select the type of memory used however.
pub struct StaticMemory {
    memory: sys::Memory,
    current: u32,
    max: Option<u32>,
}

impl StaticMemory {
    pub(in crate::memory) fn new(
        desc: MemoryDesc,
        local: &mut vm::LocalMemory,
    ) -> Option<Box<Self>> {
        let memory = {
            let mut memory =
                sys::Memory::with_size(SAFE_STATIC_HEAP_SIZE + SAFE_STATIC_GUARD_SIZE).ok()?;
            if desc.min != 0 {
                unsafe {
                    memory
                        .protect(
                            0..(desc.min as usize * WASM_PAGE_SIZE),
                            sys::Protect::ReadWrite,
                        )
                        .ok()?;
                }
            }

            memory
        };

        let mut storage = Box::new(StaticMemory {
            memory,
            current: desc.min,
            max: desc.max,
        });
        let storage_ptr: *mut StaticMemory = &mut *storage;

        local.base = storage.memory.as_ptr();
        local.bound = desc.min as usize * WASM_PAGE_SIZE;
        local.memory = storage_ptr as *mut ();

        Some(storage)
    }

    pub fn current(&self) -> u32 {
        self.current
    }

    pub fn grow(&mut self, delta: u32, local: &mut vm::LocalMemory) -> Option<u32> {
        if delta == 0 {
            return Some(self.current);
        }

        let new_pages = self.current.checked_add(delta)?;

        if let Some(max) = self.max {
            if new_pages > max {
                return None;
            }
        }

        if new_pages as usize > WASM_MAX_PAGES {
            return None;
        }

        unsafe {
            self.memory
                .protect(
                    self.current as usize * WASM_PAGE_SIZE..new_pages as usize * WASM_PAGE_SIZE,
                    sys::Protect::ReadWrite,
                )
                .ok()?;
        }

        local.bound = new_pages as usize * WASM_PAGE_SIZE;

        let old_pages = self.current;

        self.current = new_pages;

        Some(old_pages)
    }

    pub fn as_slice(&self) -> &[u8] {
        unsafe { &self.memory.as_slice()[0..self.current as usize * WASM_PAGE_SIZE] }
    }

    pub fn as_slice_mut(&mut self) -> &mut [u8] {
        unsafe { &mut self.memory.as_slice_mut()[0..self.current as usize * WASM_PAGE_SIZE] }
    }
}
