use libc::printf as _printf;

use wasmer_runtime_core::vm::Ctx;

/// putchar
pub fn putchar(ctx: &mut Ctx, chr: i32) {
    unsafe { libc::putchar(chr) };
}

/// printf
pub fn printf(ctx: &mut Ctx, memory_offset: i32, extra: i32) -> i32 {
    debug!("emscripten::printf {}, {}", memory_offset, extra);
    unsafe {
        let addr = emscripten_memory_pointer!(ctx.memory(0), memory_offset) as _;
        _printf(addr, extra)
    }
}
