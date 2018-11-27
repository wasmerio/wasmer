use super::process::abort_with_message;
use crate::webassembly::Instance;
use libc::{c_void, memcpy, size_t};

/// emscripten: _emscripten_memcpy_big
pub extern "C" fn _emscripten_memcpy_big(
    dest: u32,
    src: u32,
    len: u32,
    instance: &mut Instance,
) -> u32 {
    debug!("emscripten::_emscripten_memcpy_big");
    let dest_addr = instance.memory_offset_addr(0, dest as usize) as *mut c_void;
    let src_addr = instance.memory_offset_addr(0, src as usize) as *mut c_void;
    unsafe {
        memcpy(dest_addr, src_addr, len as size_t);
    }
    dest
}

/// emscripten: getTotalMemory
pub extern "C" fn get_total_memory(_instance: &mut Instance) -> u32 {
    debug!("emscripten::get_total_memory");
    // instance.memories[0].current_pages()
    16777216
}

/// emscripten: enlargeMemory
pub extern "C" fn enlarge_memory(_instance: &mut Instance) {
    debug!("emscripten::enlarge_memory");
    // instance.memories[0].grow(100);
}

/// emscripten: abortOnCannotGrowMemory
pub extern "C" fn abort_on_cannot_grow_memory() {
    debug!("emscripten::abort_on_cannot_grow_memory");
    abort_with_message("Cannot enlarge memory arrays!");
}
