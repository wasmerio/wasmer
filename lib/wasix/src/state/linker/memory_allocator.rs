use std::cmp::{Ord, PartialEq, PartialOrd};

use tracing::trace;
use wasmer::{AsStoreMut, Memory, MemoryError};

const WASM_PAGE_SIZE: u32 = wasmer::WASM_PAGE_SIZE as u32;

struct AllocatedPage {
    // The base_ptr is mutable, and will move forward as memory is allocated from the page.
    base_ptr: u32,

    // The amount of memory remaining until the end of the allocated region. Despite the
    // name of this struct, the region does not have to be only one page.
    remaining: u32,
}

// Used to allocate and manage memory for dynamic modules that are loaded in or
// out, since each module may request a specific amount of memory to be allocated
// for it before starting it up.
// TODO: Only supports Memory32, should implement proper Memory64 support
pub(super) struct MemoryAllocator {
    allocated_pages: Vec<AllocatedPage>,
}

impl MemoryAllocator {
    pub fn new() -> Self {
        Self {
            allocated_pages: vec![],
        }
    }

    pub fn allocate(
        &mut self,
        memory: &Memory,
        store: &mut impl AsStoreMut,
        size: u32,
        alignment: u32,
    ) -> Result<u32, MemoryError> {
        match self.allocate_in_existing_pages(size, alignment) {
            Some(base_ptr) => Ok(base_ptr),
            None => self.allocate_new_page(memory, store, size),
        }
    }

    // Finds a page which has enough free memory for the request, and allocates in it.
    // Returns the address of the allocated region if one was found.
    fn allocate_in_existing_pages(&mut self, size: u32, alignment: u32) -> Option<u32> {
        // A type to hold intermediate search results. The idea is to allocate on the page
        // that has the least amount of free space, so we can later satisfy larger allocation
        // requests without having to allocate entire new pages.
        struct CandidatePage {
            index: usize,
            base_ptr: u32,
            to_add: u32,
            remaining_free: u32,
        }

        impl PartialEq for CandidatePage {
            fn eq(&self, other: &Self) -> bool {
                self.remaining_free == other.remaining_free
            }
        }

        impl Eq for CandidatePage {}

        impl PartialOrd for CandidatePage {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }

        impl Ord for CandidatePage {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                self.remaining_free.cmp(&other.remaining_free)
            }
        }

        let mut candidates = std::collections::BinaryHeap::new();

        for (index, page) in self.allocated_pages.iter().enumerate() {
            // Offset for proper alignment
            let offset = if page.base_ptr % alignment == 0 {
                0
            } else {
                alignment - (page.base_ptr % alignment)
            };

            if page.remaining >= offset + size {
                candidates.push(std::cmp::Reverse(CandidatePage {
                    index,
                    base_ptr: page.base_ptr + offset,
                    to_add: offset + size,
                    remaining_free: page.remaining - offset - size,
                }));
            }
        }

        candidates.pop().map(|elected| {
            let page = &mut self.allocated_pages[elected.0.index];

            trace!(
                free = page.remaining,
                base_ptr = elected.0.base_ptr,
                "Found existing memory page with sufficient space"
            );

            page.base_ptr += elected.0.to_add;
            page.remaining -= elected.0.to_add;
            elected.0.base_ptr
        })
    }

    fn allocate_new_page(
        &mut self,
        memory: &Memory,
        store: &mut impl AsStoreMut,
        size: u32,
    ) -> Result<u32, MemoryError> {
        // No need to account for alignment here, as pages are already 64k-aligned
        let to_grow = size.div_ceil(WASM_PAGE_SIZE);
        let pages = memory.grow(store, to_grow)?;

        let base_ptr = pages.0 * WASM_PAGE_SIZE;
        let total_allocated = to_grow * WASM_PAGE_SIZE;

        // The initial size bytes are already allocated, rest goes into the list
        if total_allocated > size {
            self.allocated_pages.push(AllocatedPage {
                base_ptr: base_ptr + size,
                remaining: total_allocated - size,
            });
        }

        trace!(
            page_count = to_grow,
            size, base_ptr, "Allocated new memory page(s) to accommodate requested memory"
        );

        Ok(base_ptr)
    }
}

#[cfg(test)]
mod tests {
    use super::{MemoryAllocator, WASM_PAGE_SIZE};
    use wasmer::{Engine, Memory, Store};

    #[test]
    fn test_memory_allocator() {
        let engine = Engine::default();
        let mut store = Store::new(engine);
        let memory = Memory::new(
            &mut store,
            wasmer::MemoryType {
                minimum: wasmer::Pages(2),
                maximum: None,
                shared: true,
            },
        )
        .unwrap();
        let mut allocator = MemoryAllocator::new();

        // Small allocation in new page
        let addr = allocator.allocate(&memory, &mut store, 24, 4).unwrap();
        assert_eq!(addr, 2 * WASM_PAGE_SIZE);
        assert_eq!(memory.grow(&mut store, 0).unwrap().0, 3);

        // Small allocation in existing page
        let addr = allocator.allocate(&memory, &mut store, 16, 4).unwrap();
        assert_eq!(addr, 2 * WASM_PAGE_SIZE + 24);

        // Small allocation in existing page, with bigger alignment
        let addr = allocator.allocate(&memory, &mut store, 64, 32).unwrap();
        assert_eq!(addr, 2 * WASM_PAGE_SIZE + 64);
        // Should still have 3 pages
        assert_eq!(memory.grow(&mut store, 0).unwrap().0, 3);

        // Big allocation in new pages
        let addr = allocator
            .allocate(&memory, &mut store, 2 * WASM_PAGE_SIZE + 256, 1024)
            .unwrap();
        assert_eq!(addr, WASM_PAGE_SIZE * 3);
        assert_eq!(memory.grow(&mut store, 0).unwrap().0, 6);

        // Small allocation with multiple empty pages
        // page 2 has 128 bytes allocated, page 5 has 256, allocation should go
        // to page 5 (we should allocate from the page with the least free space)
        let addr = allocator
            .allocate(&memory, &mut store, 1024 * 63, 64)
            .unwrap();
        assert_eq!(addr, 5 * WASM_PAGE_SIZE + 256);

        // Another small allocation, but this time it won't fit on page 5
        let addr = allocator.allocate(&memory, &mut store, 4096, 512).unwrap();
        assert_eq!(addr, 2 * WASM_PAGE_SIZE + 512);
    }
}
