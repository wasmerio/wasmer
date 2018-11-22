use libc::printf as _printf;

use crate::webassembly::Instance;

/// putchar
pub use libc::putchar;

/// printf
pub extern "C" fn printf(memory_offset: i32, extra: i32, instance: &Instance) -> i32 {
    debug!("emscripten::printf");
    let mem = &instance.memories[0];
    return unsafe {
        let base_memory_offset = mem.mmap.as_ptr().offset(memory_offset as isize) as *const i8;
        _printf(base_memory_offset, extra)
    };
}
