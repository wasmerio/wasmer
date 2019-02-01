use libc::printf as _printf;

use wasmer_runtime_core::vm::Ctx;

/// putchar
pub extern "C" fn putchar(chr: i32, ctx: &mut Ctx) {
    unsafe { libc::putchar(chr) };
}

/// printf
pub extern "C" fn printf(memory_offset: i32, extra: i32, ctx: &mut Ctx) -> i32 {
    debug!("emscripten::printf {}, {}", memory_offset, extra);
    unsafe {
        let addr = emscripten_memory_pointer!(ctx.memory(0), memory_offset) as _;
        _printf(addr, extra)
    }
}
