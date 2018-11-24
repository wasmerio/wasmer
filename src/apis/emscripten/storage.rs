
use crate::webassembly::{LinearMemory, Instance};

<<<<<<< HEAD
pub fn align_memory(ptr: u32) -> u32 {
    (ptr + 15) & !15
=======
pub fn align_memory(size: u32, factor: u32) -> u32 {
    assert!(factor != 0, "memory cannot be aligned by 0 offset!");
    if size % factor == 1 {
        (size) - (size % factor) + (factor)
    } else {
        size
    }
>>>>>>> Add some syscalls
}

// pub fn static_alloc(size: u32, instance: &mut Instance) -> u32 {
//     let static_top = instance.emscripten_data.static_top;
//     let total_memory = instance.memories[0].maximum.unwrap_or(LinearMemory::DEFAULT_HEAP_SIZE as u32);
//     instance.emscripten_data.static_top = (static_top + size + 15) & 4294967280;
//     assert!(static_top < total_memory, "not enough memory for static allocation - increase total_memory!");
//     static_top
// }

pub fn static_alloc(size: u32, static_top: &mut u32, memory: &LinearMemory) -> u32 {
    let old_static_top = *static_top;
<<<<<<< HEAD
    let total_memory = memory.maximum_size() * LinearMemory::PAGE_SIZE;
=======
    let total_memory = memory.maximum.unwrap_or(LinearMemory::MAX_PAGES as u32) * LinearMemory::PAGE_SIZE;
>>>>>>> Add some syscalls
    // NOTE: The `4294967280` is a u32 conversion of -16 as gotten from emscripten.
    *static_top = (*static_top + size + 15) & 4294967280;
    assert!(*static_top < total_memory, "not enough memory for static allocation - increase total_memory!");
    old_static_top
}
