use crate::varargs::VarArgs;
use libc::mkdir;
use std::os::raw::c_int;
use wasmer_runtime_core::vm::Ctx;

type pid_t = c_int;

// chown
pub fn ___syscall212(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall212 (chown) {}", which);
    -1
}

// mkdir
pub fn ___syscall39(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall39 (mkdir) {}", which);
    let pathname: u32 = varargs.get(ctx);
    let mode: u32 = varargs.get(ctx);
    let pathname_addr = emscripten_memory_pointer!(ctx.memory(0), pathname) as *const i8;
    unsafe { mkdir(pathname_addr) }
}

// getgid
pub fn ___syscall201(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall201 (getgid)");
    -1
}

// getgid32
pub fn ___syscall202(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    // gid_t
    debug!("emscripten::___syscall202 (getgid32)");
    -1
}

/// dup3
pub fn ___syscall330(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> pid_t {
    debug!("emscripten::___syscall330 (dup3)");
    -1
}

/// ioctl
pub fn ___syscall54(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall54 (ioctl) {}", which);
    -1
}

// socketcall
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall102(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall102 (socketcall) {}", which);
    -1
}

// pread
pub fn ___syscall180(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall180 (pread) {}", which);
    -1
}

// pwrite
pub fn ___syscall181(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall181 (pwrite) {}", which);
    -1
}

/// wait4
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall114(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> pid_t {
    debug!("emscripten::___syscall114 (wait4)");
    -1
}

// select
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall142(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall142 (newselect) {}", which);
    -1
}

// setpgid
pub fn ___syscall57(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall57 (setpgid) {}", which);
    -1
}

/// uname
// NOTE: Wondering if we should return custom utsname, like Emscripten.
pub fn ___syscall122(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall122 (uname) {}", which);
    -1
}
