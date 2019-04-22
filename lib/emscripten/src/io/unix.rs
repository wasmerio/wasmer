use libc::{chroot as _chroot, printf as _printf};

use wasmer_runtime_core::vm::Ctx;

/// putchar
pub fn putchar(_ctx: &mut Ctx, chr: i32) {
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

/// chroot
pub fn chroot(ctx: &mut Ctx, name_ptr: i32) -> i32 {
    debug!("emscripten::chroot");
    let name = emscripten_memory_pointer!(ctx.memory(0), name_ptr) as *const i8;
    unsafe { _chroot(name) }
}

/// getpwuid
pub fn getpwuid(_ctx: &mut Ctx, _uid: i32) -> i32 {
    debug!("emscripten::getpwuid");
    0
}
