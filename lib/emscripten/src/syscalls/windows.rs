use crate::env::get_emscripten_data;
use crate::utils::copy_cstr_into_wasm;
use crate::varargs::VarArgs;
use libc::mkdir;
use libc::open;
use rand::Rng;
use std::env;
use std::ffi::CString;
use std::fs::File;
use std::io::Write;
use std::os::raw::c_int;
use wasmer_runtime_core::vm::Ctx;

#[allow(non_camel_case_types)]
type pid_t = c_int;

/// open
#[cfg(not(feature = "vfs"))]
pub fn ___syscall5(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall5 (open) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    let pathname: u32 = varargs.get(ctx);
    let flags: i32 = varargs.get(ctx);
    let mode: u32 = varargs.get(ctx);
    let pathname_addr = emscripten_memory_pointer!(ctx.memory(0), pathname) as *const i8;
    let path_str = unsafe { std::ffi::CStr::from_ptr(pathname_addr).to_str().unwrap() };
    match path_str {
        "/dev/urandom" => {
            // create a fake urandom file for windows, super hacky
            // put it in the temp directory so we can just forget about it
            let mut tmp_dir = env::temp_dir();
            tmp_dir.push("urandom");
            let tmp_dir_str = tmp_dir.to_str().unwrap();
            let tmp_dir_c_str = CString::new(tmp_dir_str).unwrap();
            let ptr = tmp_dir_c_str.as_ptr() as *const i8;
            let mut urandom_file = File::create(tmp_dir).unwrap();
            // create some random bytes and put them into the file
            let random_bytes = rand::thread_rng().gen::<[u8; 32]>();
            let _ = urandom_file.write_all(&random_bytes).unwrap();
            // put the file path string into wasm memory
            let urandom_file_offset = unsafe { copy_cstr_into_wasm(ctx, ptr) };
            let raw_pointer_to_urandom_file =
                emscripten_memory_pointer!(ctx.memory(0), urandom_file_offset) as *const i8;
            let fd = unsafe { open(raw_pointer_to_urandom_file, flags, mode) };
            debug!(
                "=> pathname: {}, flags: {}, mode: {} = fd: {}",
                pathname, flags, mode, fd
            );
            fd
        }
        _ => {
            let fd = unsafe { open(pathname_addr, flags, mode) };
            debug!(
                "=> pathname: {}, flags: {}, mode: {} = fd: {}\npath: {}",
                pathname, flags, mode, fd, path_str
            );
            fd
        }
    }
}

// chown
pub fn ___syscall212(_ctx: &mut Ctx, which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall212 (chown) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    -1
}

// mkdir
#[cfg(not(feature = "vfs"))]
pub fn ___syscall39(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall39 (mkdir) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    let pathname: u32 = varargs.get(ctx);
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
pub fn ___syscall330(_ctx: &mut Ctx, _which: c_int, mut _varargs: VarArgs) -> pid_t {
    debug!("emscripten::___syscall330 (dup3)");
    -1
}

/// ioctl
pub fn ___syscall54(_ctx: &mut Ctx, which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall54 (ioctl) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    -1
}

// socketcall
#[cfg(not(feature = "vfs"))]
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall102(_ctx: &mut Ctx, which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall102 (socketcall) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    -1
}

/// pread
#[cfg(not(feature = "vfs"))]
pub fn ___syscall180(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall180 (pread) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    -1
}

/// pwrite
#[cfg(not(feature = "vfs"))]
pub fn ___syscall181(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall181 (pwrite) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    -1
}

/// wait4
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall114(_ctx: &mut Ctx, _which: c_int, mut _varargs: VarArgs) -> pid_t {
    debug!("emscripten::___syscall114 (wait4)");
    -1
}

// select
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall142(_ctx: &mut Ctx, which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall142 (newselect) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    -1
}

// setpgid
pub fn ___syscall57(_ctx: &mut Ctx, which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall57 (setpgid) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    -1
}

/// uname
// NOTE: Wondering if we should return custom utsname, like Emscripten.
pub fn ___syscall122(_ctx: &mut Ctx, which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall122 (uname) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    -1
}
