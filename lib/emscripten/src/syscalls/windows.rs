use crate::utils::{copy_cstr_into_wasm, get_cstr_path};
use crate::varargs::VarArgs;
use crate::EmEnv;
use libc::mkdir;
use libc::open;
use std::env;
use std::ffi::CString;
use std::fs::File;
use std::io::Write;
use std::os::raw::c_int;
use wasmer::FunctionEnvMut;

#[allow(non_camel_case_types)]
type pid_t = c_int;

/// open
pub fn ___syscall5(mut ctx: FunctionEnvMut<EmEnv>, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall5 (open) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    let pathname_addr = varargs.get_str(&ctx);
    let real_path_owned = get_cstr_path(ctx.as_mut(), pathname_addr);
    let real_path = if let Some(ref rp) = real_path_owned {
        rp.as_c_str().as_ptr()
    } else {
        pathname_addr
    };
    let flags: i32 = varargs.get(&ctx);
    let mode: u32 = varargs.get(&ctx);
    let path_str = unsafe { std::ffi::CStr::from_ptr(real_path).to_str().unwrap() };

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
            let mut random_bytes = [0u8; 32];
            getrandom::getrandom(&mut random_bytes).unwrap();
            let _ = urandom_file.write_all(&random_bytes).unwrap();
            // put the file path string into wasm memory
            let urandom_file_offset = unsafe { copy_cstr_into_wasm(&mut ctx, ptr) };
            let mem = ctx.data().memory(0);
            let memory = mem.view(&ctx);
            let raw_pointer_to_urandom_file =
                emscripten_memory_pointer!(&memory, urandom_file_offset) as *const i8;
            let fd = unsafe { open(raw_pointer_to_urandom_file, flags, mode) };
            debug!(
                "=> pathname: {}, flags: {}, mode: {} = fd: {}",
                path_str, flags, mode, fd
            );
            fd
        }
        _ => {
            let fd = unsafe { open(real_path, flags, mode) };
            debug!(
                "=> pathname: {}, flags: {}, mode: {} = fd: {}\npath: {}",
                path_str, flags, mode, fd, path_str
            );
            fd
        }
    }
}

/// link
pub fn ___syscall9(_ctx: FunctionEnvMut<EmEnv>, _which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall9 (link) {}", _which);
    unimplemented!("emscripten::___syscall9 (link) {}", _which);
}

/// ftruncate64
pub fn ___syscall194(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall194 - stub");
    unimplemented!("emscripten::___syscall194 - stub")
}

// chown
pub fn ___syscall212(_ctx: FunctionEnvMut<EmEnv>, which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall212 (chown) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    -1
}

/// access
pub fn ___syscall33(_ctx: FunctionEnvMut<EmEnv>, _which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall33 (access) {}", _which);
    unimplemented!("emscripten::___syscall33 (access) {}", _which);
}

/// nice
pub fn ___syscall34(_ctx: FunctionEnvMut<EmEnv>, _which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall34 (nice) {}", _which);
    unimplemented!("emscripten::___syscall34 (nice) {}", _which);
}

// mkdir
pub fn ___syscall39(ctx: FunctionEnvMut<EmEnv>, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall39 (mkdir) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    let pathname_addr = varargs.get_str(&ctx);
    let real_path_owned = get_cstr_path(ctx, pathname_addr);
    let real_path = if let Some(ref rp) = real_path_owned {
        rp.as_c_str().as_ptr()
    } else {
        pathname_addr
    };
    unsafe { mkdir(real_path) }
}

/// dup
pub fn ___syscall41(_ctx: FunctionEnvMut<EmEnv>, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall41 (dup) {}", _which);
    unimplemented!("emscripten::___syscall41 (dup) {}", _which);
}

/// getrusage
pub fn ___syscall77(_ctx: FunctionEnvMut<EmEnv>, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall77 (getrusage) {}", _which);
    unimplemented!("emscripten::___syscall77 (getrusage) {}", _which);
}

/// symlink
pub fn ___syscall83(_ctx: FunctionEnvMut<EmEnv>, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall83 (symlink) {}", _which);
    unimplemented!("emscripten::___syscall83 (symlink) {}", _which);
}

/// readlink
pub fn ___syscall85(_ctx: FunctionEnvMut<EmEnv>, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall85 (readlink) {}", _which);
    -1
}

/// getpgid
pub fn ___syscall132(_ctx: FunctionEnvMut<EmEnv>, _which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall132 (getpgid)");
    -1
}

/// lchown
pub fn ___syscall198(_ctx: FunctionEnvMut<EmEnv>, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall198 (lchown) {}", _which);
    unimplemented!("emscripten::___syscall198 (lchown) {}", _which);
}

/// getgid32
pub fn ___syscall200(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall200 (getgid32)");
    unimplemented!("emscripten::___syscall200 (getgid32)");
}

// geteuid32
pub fn ___syscall201(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall201 (geteuid32)");
    unimplemented!("emscripten::___syscall201 (geteuid32)");
}

// getegid32
pub fn ___syscall202(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    // gid_t
    debug!("emscripten::___syscall202 (getegid32)");
    unimplemented!("emscripten::___syscall202 (getegid32)");
}

/// getgroups
pub fn ___syscall205(_ctx: FunctionEnvMut<EmEnv>, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall205 (getgroups) {}", _which);
    unimplemented!("emscripten::___syscall205 (getgroups) {}", _which);
}

/// madvise
pub fn ___syscall219(_ctx: FunctionEnvMut<EmEnv>, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall212 (chown) {}", _which);
    unimplemented!("emscripten::___syscall212 (chown) {}", _which);
}

/// dup3
pub fn ___syscall330(_ctx: FunctionEnvMut<EmEnv>, _which: c_int, mut _varargs: VarArgs) -> pid_t {
    debug!("emscripten::___syscall330 (dup3)");
    -1
}

/// ioctl
pub fn ___syscall54(_ctx: FunctionEnvMut<EmEnv>, which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall54 (ioctl) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    -1
}

/// fchmod
pub fn ___syscall94(_ctx: FunctionEnvMut<EmEnv>, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall118 (fchmod) {}", _which);
    unimplemented!("emscripten::___syscall118 (fchmod) {}", _which);
}

// socketcall
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall102(_ctx: FunctionEnvMut<EmEnv>, which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall102 (socketcall) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    -1
}

/// fsync
pub fn ___syscall118(_ctx: FunctionEnvMut<EmEnv>, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall118 (fsync) {}", _which);
    unimplemented!("emscripten::___syscall118 (fsync) {}", _which);
}

// pread
pub fn ___syscall180(_ctx: FunctionEnvMut<EmEnv>, which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall180 (pread) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    -1
}

// pwrite
pub fn ___syscall181(_ctx: FunctionEnvMut<EmEnv>, which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall181 (pwrite) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    -1
}

/// wait4
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall114(_ctx: FunctionEnvMut<EmEnv>, _which: c_int, mut _varargs: VarArgs) -> pid_t {
    debug!("emscripten::___syscall114 (wait4)");
    -1
}

// select
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall142(_ctx: FunctionEnvMut<EmEnv>, which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall142 (newselect) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    -1
}

/// fdatasync
pub fn ___syscall148(_ctx: FunctionEnvMut<EmEnv>, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall148 (fdatasync) {}", _which);
    unimplemented!("emscripten::___syscall148 (fdatasync) {}", _which);
}

// setpgid
pub fn ___syscall57(_ctx: FunctionEnvMut<EmEnv>, which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall57 (setpgid) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    -1
}

/// uname
// NOTE: Wondering if we should return custom utsname, like Emscripten.
pub fn ___syscall122(_ctx: FunctionEnvMut<EmEnv>, which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall122 (uname) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    -1
}

/// poll
pub fn ___syscall168(_ctx: FunctionEnvMut<EmEnv>, _which: i32, _varargs: VarArgs) -> i32 {
    debug!("emscripten::___syscall168(poll) - stub");
    -1
}

/// lstat64
pub fn ___syscall196(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall196 (lstat64) - stub");
    -1
}

// getuid
pub fn ___syscall199(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall199 (getuid)");
    -1
}

// getdents
pub fn ___syscall220(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall220");
    -1
}

// fcntl64
pub fn ___syscall221(_ctx: FunctionEnvMut<EmEnv>, _which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall221 (fcntl64) {}", _which);
    -1
}

/// fchown
pub fn ___syscall207(_ctx: FunctionEnvMut<EmEnv>, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall207 (fchown) {}", _which);
    unimplemented!("emscripten::___syscall207 (fchown) {}", _which)
}

/// fallocate
pub fn ___syscall324(_ctx: FunctionEnvMut<EmEnv>, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall324 (fallocate) {}", _which);
    unimplemented!("emscripten::___syscall324 (fallocate) {}", _which)
}
