use crate::varargs::VarArgs;
use libc::mkdir;
use std::os::raw::c_int;
use wasmer_runtime_core::vm::Ctx;

type pid_t = c_int;

// chown
pub fn ___syscall212(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall212 (chown) {}", which);
    -1
}

// mkdir
pub fn ___syscall39(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall39 (mkdir) {}", which);
    let pathname: u32 = varargs.get(ctx);
    let mode: u32 = varargs.get(ctx);
    let pathname_addr = emscripten_memory_pointer!(ctx.memory(0), pathname) as *const i8;
    unsafe { mkdir(pathname_addr) }
}

// getgid
pub fn ___syscall201(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall201 (getgid)");
    -1
}

// getgid32
pub fn ___syscall202(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    // gid_t
    debug!("emscripten::___syscall202 (getgid32)");
    -1
}

/// dup3
pub fn ___syscall330(_which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> pid_t {
    debug!("emscripten::___syscall330 (dup3)");
    -1
}

/// ioctl
pub fn ___syscall54(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall54 (ioctl) {}", which);
    -1
}

// socketcall
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall102(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall102 (socketcall) {}", which);
    -1
}

// pread
pub fn ___syscall180(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall180 (pread) {}", which);
    -1
}

// pwrite
pub fn ___syscall181(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall181 (pwrite) {}", which);
    -1
}

/// wait4
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall114(_which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> pid_t {
    debug!("emscripten::___syscall114 (wait4)");
    -1
}

// select
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall142(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall142 (newselect) {}", which);
    -1
}

// setpgid
pub fn ___syscall57(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall57 (setpgid) {}", which);
    -1
}

/// uname
// NOTE: Wondering if we should return custom utsname, like Emscripten.
pub fn ___syscall122(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall122 (uname) {}", which);
    -1
}
