use crate::{
    memory::{WASM_MAX_PAGES, WASM_PAGE_SIZE},
    sys,
    types::MemoryDesc,
    vm,
};

pub const DYNAMIC_GUARD_SIZE: usize = 4096;

/// This is an internal-only api.
///
/// A Dynamic memory allocates only the minimum amount of memory
/// when first created. Over time, as it grows, it may reallocate to
/// a different location and size.
///
/// Dynamic memories are signifigantly faster to create than static
/// memories and use much less virtual memory, however, they require
/// the webassembly module to bounds-check memory accesses.
///
/// While, a dynamic memory could use a vector of some sort as its
/// backing memory, we use mmap (or the platform-equivalent) to allow
/// us to add a guard-page at the end to help elide some bounds-checks.
pub struct DynamicMemory {
    memory: sys::Memory,
    current: u32,
    max: Option<u32>,
}

impl DynamicMemory {
    pub(super) fn new(desc: MemoryDesc, local: &mut vm::LocalMemory) -> Option<Box<Self>> {
        let memory = {
            let mut memory =
                sys::Memory::with_size((desc.min as usize * WASM_PAGE_SIZE) + DYNAMIC_GUARD_SIZE)
                    .ok()?;
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

        let mut storage = Box::new(DynamicMemory {
            memory,
            current: desc.min,
            max: desc.max,
        });
        let storage_ptr: *mut DynamicMemory = &mut *storage;

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

        let mut new_memory =
            sys::Memory::with_size((new_pages as usize * WASM_PAGE_SIZE) + DYNAMIC_GUARD_SIZE)
                .ok()?;

        unsafe {
            new_memory
                .protect(
                    0..(new_pages as usize * WASM_PAGE_SIZE),
                    sys::Protect::ReadWrite,
                )
                .ok()?;

            new_memory.as_slice_mut()[..self.current as usize * WASM_PAGE_SIZE]
                .copy_from_slice(&self.memory.as_slice()[..self.current as usize * WASM_PAGE_SIZE]);
        }

        self.memory = new_memory; //The old memory gets dropped.

        local.base = self.memory.as_ptr();
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
