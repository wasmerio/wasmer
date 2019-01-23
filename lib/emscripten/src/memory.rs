use super::process::abort_with_message;
use libc::{c_int, c_void, memcpy, size_t};
use wasmer_runtime_core::vm::Ctx;

/// emscripten: _emscripten_memcpy_big
pub extern "C" fn _emscripten_memcpy_big(dest: u32, src: u32, len: u32, vmctx: &mut Ctx) -> u32 {
    debug!(
        "emscripten::_emscripten_memcpy_big {}, {}, {}",
        dest, src, len
    );
    let dest_addr = vmctx.memory(0)[dest as usize] as *mut c_void;
    let src_addr = vmctx.memory(0)[src as usize] as *mut c_void;
    unsafe {
        memcpy(dest_addr, src_addr, len as size_t);
    }
    dest
}

/// emscripten: getTotalMemory
pub extern "C" fn get_total_memory(_vmctx: &mut Ctx) -> u32 {
    debug!("emscripten::get_total_memory");
    // instance.memories[0].current_pages()
    // TODO: Fix implementation
    16_777_216
}

/// emscripten: enlargeMemory
pub extern "C" fn enlarge_memory(_vmctx: &mut Ctx) {
    debug!("emscripten::enlarge_memory");
    // instance.memories[0].grow(100);
    // TODO: Fix implementation
}

/// emscripten: abortOnCannotGrowMemory
pub extern "C" fn abort_on_cannot_grow_memory() {
    debug!("emscripten::abort_on_cannot_grow_memory");
    abort_with_message("Cannot enlarge memory arrays!");
}

/// emscripten: ___map_file
pub extern "C" fn ___map_file() -> c_int {
    debug!("emscripten::___map_file");
    // NOTE: TODO: Em returns -1 here as well. May need to implement properly
    -1
}
