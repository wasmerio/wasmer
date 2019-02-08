// use crate::webassembly::Memory;

pub fn align_memory(ptr: u32) -> u32 {
    (ptr + 15) & !15
}

pub fn static_alloc(static_top: &mut u32, size: u32) -> u32 {
    let old_static_top = *static_top;
    // NOTE: The `4294967280` is a u32 conversion of -16 as gotten from emscripten.
    *static_top = (*static_top + size + 15) & 4294967280;
    old_static_top
}
