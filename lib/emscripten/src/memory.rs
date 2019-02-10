use super::process::abort_with_message;
use libc::{c_int, c_void, memcpy, size_t};
use wasmer_runtime_core::vm::Ctx;

/// emscripten: _emscripten_memcpy_big
pub fn _emscripten_memcpy_big(ctx: &mut Ctx, dest: u32, src: u32, len: u32) -> u32 {
    debug!(
        "emscripten::_emscripten_memcpy_big {}, {}, {}",
        dest, src, len
    );
    let dest_addr = emscripten_memory_pointer!(ctx.memory(0), dest) as *mut c_void;
    let src_addr = emscripten_memory_pointer!(ctx.memory(0), src) as *mut c_void;
    unsafe {
        memcpy(dest_addr, src_addr, len as size_t);
    }
    dest
}

/// emscripten: _emscripten_get_heap_size
pub fn _emscripten_get_heap_size(ctx: &mut Ctx) -> u32 {
    debug!("emscripten::_emscripten_get_heap_size",);
    // TODO: Fix implementation
    16_777_216
}

/// emscripten: _emscripten_resize_heap
pub fn _emscripten_resize_heap(ctx: &mut Ctx, requested_size: u32) -> u32 {
    debug!("emscripten::_emscripten_resize_heap {}", requested_size);
    // TODO: Fix implementation
    0
}

/// emscripten: getTotalMemory
pub fn get_total_memory(_ctx: &mut Ctx) -> u32 {
    debug!("emscripten::get_total_memory");
    // instance.memories[0].current_pages()
    // TODO: Fix implementation
    16_777_216
}

/// emscripten: enlargeMemory
pub fn enlarge_memory(_ctx: &mut Ctx) -> u32 {
    debug!("emscripten::enlarge_memory");
    // instance.memories[0].grow(100);
    // TODO: Fix implementation
    0
}

/// emscripten: abortOnCannotGrowMemory
pub fn abort_on_cannot_grow_memory(ctx: &mut Ctx, requested_size: u32) -> u32 {
    debug!("emscripten::abort_on_cannot_grow_memory {}", requested_size);
    abort_with_message(ctx, "Cannot enlarge memory arrays!");
    0
}

/// emscripten: abortOnCannotGrowMemory
pub fn abort_on_cannot_grow_memory_old(ctx: &mut Ctx) -> u32 {
    debug!("emscripten::abort_on_cannot_grow_memory");
    abort_with_message(ctx, "Cannot enlarge memory arrays!");
    0
}

/// emscripten: ___map_file
pub fn ___map_file(_ctx: &mut Ctx, _one: u32, _two: u32) -> c_int {
    debug!("emscripten::___map_file");
    // NOTE: TODO: Em returns -1 here as well. May need to implement properly
    -1
}
