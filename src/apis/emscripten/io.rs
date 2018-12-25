use libc::printf as _printf;

use crate::webassembly::Instance;

/// putchar
pub use libc::putchar;

/// printf
pub extern "C" fn printf(memory_offset: i32, extra: i32, instance: &Instance) -> i32 {
    debug!("emscripten::printf {}, {}", memory_offset, extra);
    unsafe {
        let addr = instance.memory_offset_addr(0, memory_offset as _) as _;
        _printf(addr, extra)
    }
}
