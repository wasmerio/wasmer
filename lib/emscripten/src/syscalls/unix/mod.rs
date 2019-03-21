use crate::varargs::VarArgs;

#[cfg(not(feature = "vfs"))]
pub mod host_fs;

#[cfg(feature = "vfs")]
pub mod vfs;

#[cfg(not(feature = "vfs"))]
pub use host_fs::*;

#[cfg(feature = "vfs")]
pub use vfs::*;

/// NOTE: TODO: These syscalls only support wasm_32 for now because they assume offsets are u32
/// Syscall list: https://www.cs.utexas.edu/~bismith/test/syscalls/syscalls32.html
use libc::{c_int, pid_t, rusage, setpgid, uname, utsname};
use wasmer_runtime_core::vm::Ctx;

// Linking to functions that are not provided by rust libc
#[cfg(target_os = "macos")]
#[link(name = "c")]
extern "C" {
    pub fn wait4(pid: pid_t, status: *mut c_int, options: c_int, rusage: *mut rusage) -> pid_t;
}

#[cfg(not(target_os = "macos"))]
use libc::wait4;
use crate::utils::read_string_from_wasm;

/// wait4
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall114(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> pid_t {
    debug!("emscripten::___syscall114 (wait4)");
    let pid: pid_t = varargs.get(ctx);
    let status: u32 = varargs.get(ctx);
    let options: c_int = varargs.get(ctx);
    let rusage: u32 = varargs.get(ctx);
    let status_addr = emscripten_memory_pointer!(ctx.memory(0), status) as *mut c_int;
    let rusage_addr = emscripten_memory_pointer!(ctx.memory(0), rusage) as *mut rusage;
    let res = unsafe { wait4(pid, status_addr, options, rusage_addr) };
    debug!(
        "=> pid: {}, status: {:?}, options: {}, rusage: {:?} = pid: {}",
        pid, status_addr, options, rusage_addr, res
    );
    res
}

/// setpgid
pub fn ___syscall57(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall57 (setpgid) {}", _which);
    let pid: i32 = varargs.get(ctx);
    let pgid: i32 = varargs.get(ctx);
    unsafe { setpgid(pid, pgid) }
}

/// uname
// NOTE: Wondering if we should return custom utsname, like Emscripten.
pub fn ___syscall122(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall122 (uname) {}", _which);
    let buf: u32 = varargs.get(ctx);
    debug!("=> buf: {}", buf);
    let buf_addr = emscripten_memory_pointer!(ctx.memory(0), buf) as *mut utsname;
    let uname_result = unsafe { uname(buf_addr) };
    debug!("uname buf: {}", read_string_from_wasm(ctx.memory(0), buf));
    uname_result
}

/// getgid
pub fn ___syscall201(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall201 (getgid)");
    let result = unsafe {
        // Maybe fix: Emscripten returns 0 always
        libc::getgid() as i32
    };
    result
}

/// getgid32
pub fn ___syscall202(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    // gid_t
    debug!("emscripten::___syscall202 (getgid32)");
    unsafe {
        // Maybe fix: Emscripten returns 0 always
        libc::getgid() as _
    }
}

/// chown
pub fn ___syscall212(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall212 (chown) {}", _which);

    let pathname: u32 = varargs.get(ctx);
    let owner: u32 = varargs.get(ctx);
    let group: u32 = varargs.get(ctx);

    let pathname_addr = emscripten_memory_pointer!(ctx.memory(0), pathname) as *const i8;

    unsafe { libc::chown(pathname_addr, owner, group) }
}
