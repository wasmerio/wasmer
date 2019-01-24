use super::process::abort_with_message;
use libc::{c_int, c_void, memcpy, size_t};
use wasmer_runtime_core::vm::Ctx;

/// emscripten: _emscripten_memcpy_big
pub extern "C" fn _emscripten_memcpy_big(dest: u32, src: u32, len: u32, ctx: &mut Ctx) -> u32 {
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

/// emscripten: getTotalMemory
pub extern "C" fn get_total_memory(_ctx: &mut Ctx) -> u32 {
    debug!("emscripten::get_total_memory");
    // instance.memories[0].current_pages()
    // TODO: Fix implementation
    16_777_216
}

/// emscripten: enlargeMemory
pub extern "C" fn enlarge_memory(_ctx: &mut Ctx) -> u32 {
    debug!("emscripten::enlarge_memory");
    // instance.memories[0].grow(100);
    // TODO: Fix implementation
    0
}

/// emscripten: abortOnCannotGrowMemory
pub extern "C" fn abort_on_cannot_grow_memory(_ctx: &mut Ctx) -> u32 {
    debug!("emscripten::abort_on_cannot_grow_memory");
    abort_with_message("Cannot enlarge memory arrays!");
    0
}

/// emscripten: ___map_file
pub extern "C" fn ___map_file(_ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___map_file");
    // NOTE: TODO: Em returns -1 here as well. May need to implement properly
    -1
}
