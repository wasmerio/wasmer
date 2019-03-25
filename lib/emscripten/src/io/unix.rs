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
    let name = emscripten_memory_pointer!(ctx.memory(0), name_ptr) as *const i8;
    unsafe { _chroot(name) }
}

/// getprotobyname
pub fn getprotobyname(ctx: &mut Ctx, name_ptr: i32) -> i32 {
    debug!("emscripten::getprotobyname");
    // TODO: actually do this logic to return correctly
    let _name = emscripten_memory_pointer!(ctx.memory(0), name_ptr) as *const i8;
    //unsafe { _getprotobyname(name) as i32 }
    0
}

/// getprotobynumber
pub fn getprotobynumber(_ctx: &mut Ctx, _one: i32) -> i32 {
    debug!("emscripten::getprotobynumber");
    0
}

/// getpwuid
pub fn getpwuid(_ctx: &mut Ctx, _uid: i32) -> i32 {
    debug!("emscripten::getpwuid");
    // TODO: actually do this logic to return correctly
    0
}

/// longjmp
pub fn longjmp(_ctx: &mut Ctx, _one: i32, _two: i32) {
    debug!("emscripten::longjump");
}

/// sigdelset
pub fn sigdelset(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::sigdelset");
    0
}

/// sigfillset
pub fn sigfillset(_ctx: &mut Ctx, _one: i32) -> i32 {
    debug!("emscripten::sigfillset");
    0
}

/// tzset
pub fn tzset(_ctx: &mut Ctx) {
    debug!("emscripten::tzset");
}

/// strptime
pub fn strptime(_ctx: &mut Ctx, _one: i32, _two: i32, _three: i32) -> i32 {
    debug!("emscripten::strptime");
    0
}
