use crate::varargs::VarArgs;

#[cfg(not(feature = "vfs"))]
pub mod host_fs;

#[cfg(all(not(target_os = "windows"), feature = "vfs"))]
pub mod vfs;

#[cfg(not(feature = "vfs"))]
pub use host_fs::*;

#[cfg(feature = "vfs")]
pub mod select;

#[cfg(feature = "vfs")]
pub use select::*;

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
    pub fn madvise(addr: *mut libc::c_void, len: libc::size_t, advice: c_int) -> c_int;
    pub fn fdatasync(fd: c_int) -> c_int;
    pub fn lstat64(path: *const libc::c_char, buf: *mut libc::c_void) -> c_int;
}

#[cfg(not(target_os = "macos"))]
use libc::{fallocate, fdatasync, ftruncate64, lstat64, madvise, wait4};

/// link
pub fn ___syscall9(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall9 (link) {}", _which);

    let oldname: c_int = varargs.get(ctx);
    let newname: c_int = varargs.get(ctx);
    let oldname_ptr = emscripten_memory_pointer!(ctx.memory(0), oldname) as *const i8;
    let newname_ptr = emscripten_memory_pointer!(ctx.memory(0), newname) as *const i8;
    let result = unsafe { libc::link(oldname_ptr, newname_ptr) };
    debug!(
        "=> oldname: {}, newname: {}, result: {}",
        unsafe { std::ffi::CStr::from_ptr(oldname_ptr).to_str().unwrap() },
        unsafe { std::ffi::CStr::from_ptr(newname_ptr).to_str().unwrap() },
        result,
    );
    result
}

/// access
pub fn ___syscall33(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall33 (access) {}", _which);
    let path_ptr: c_int = varargs.get(ctx);
    let amode: c_int = varargs.get(ctx);
    let path = emscripten_memory_pointer!(ctx.memory(0), path_ptr) as *const i8;
    let result = unsafe { libc::access(path, amode) };
    debug!(
        "=> path: {}, result: {}",
        unsafe { std::ffi::CStr::from_ptr(path).to_str().unwrap() },
        result
    );
    result
}

/// nice
pub fn ___syscall34(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall34 (nice) {}", _which);
    let inc_r: c_int = varargs.get(ctx);
    unsafe { libc::nice(inc_r) }
}

/// getrusage
pub fn ___syscall77(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall77 (getrusage) {}", _which);

    let resource: c_int = varargs.get(ctx);
    let rusage_ptr: c_int = varargs.get(ctx);
    #[allow(clippy::cast_ptr_alignment)]
    let rusage = emscripten_memory_pointer!(ctx.memory(0), rusage_ptr) as *mut rusage;
    assert_eq!(8, std::mem::align_of_val(&rusage));
    unsafe { libc::getrusage(resource, rusage) }
}

/// dup
pub fn ___syscall41(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall41 (dup) {}", _which);
    let fd: c_int = varargs.get(ctx);
    unsafe { libc::dup(fd) }
}

/// setpgid
pub fn ___syscall57(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall57 (setpgid) {}", _which);
    let pid: i32 = varargs.get(ctx);
    let pgid: i32 = varargs.get(ctx);
    unsafe { setpgid(pid, pgid) }
}

/// symlink
pub fn ___syscall83(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall83 (symlink) {}", _which);

    let path1_ptr: c_int = varargs.get(ctx);
    let path2_ptr: c_int = varargs.get(ctx);
    let path1 = emscripten_memory_pointer!(ctx.memory(0), path1_ptr) as *mut i8;
    let path2 = emscripten_memory_pointer!(ctx.memory(0), path2_ptr) as *mut i8;
    let result = unsafe { libc::symlink(path1, path2) };
    debug!(
        "=> path1: {}, path2: {}, result: {}",
        unsafe { std::ffi::CStr::from_ptr(path1).to_str().unwrap() },
        unsafe { std::ffi::CStr::from_ptr(path2).to_str().unwrap() },
        result,
    );
    result
}

/// fchmod
pub fn ___syscall94(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall118 (fchmod) {}", _which);
    let fd: c_int = varargs.get(ctx);
    let mode: libc::mode_t = varargs.get(ctx);
    unsafe { libc::fchmod(fd, mode) }
}

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

/// fsync
pub fn ___syscall118(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall118 (fsync) {}", _which);
    let fd: c_int = varargs.get(ctx);
    unsafe { libc::fsync(fd) }
}

/// uname
// NOTE: Wondering if we should return custom utsname, like Emscripten.
pub fn ___syscall122(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall122 (uname) {}", _which);
    let buf: u32 = varargs.get(ctx);
    debug!("=> buf: {}", buf);
    let buf_addr = emscripten_memory_pointer!(ctx.memory(0), buf) as *mut utsname;
    let uname_result = unsafe { uname(buf_addr) };
    debug!(
        "uname buf: {}",
        crate::utils::read_string_from_wasm(ctx.memory(0), buf)
    );
    uname_result
}

/// fdatasync
pub fn ___syscall148(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall148 (fdatasync) {}", _which);

    let fd: i32 = varargs.get(ctx);

    unsafe { fdatasync(fd) }
}

/// ftruncate64
pub fn ___syscall194(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall194 (ftruncate64) {}", _which);
    let _fd: c_int = varargs.get(ctx);
    let _length: i64 = varargs.get(ctx);
    #[cfg(not(target_os = "macos"))]
    unsafe {
        ftruncate64(_fd, _length)
    }
    #[cfg(target_os = "macos")]
    unimplemented!()
}

/// lstat64
pub fn ___syscall196(ctx: &mut Ctx, _which: i32, mut varargs: VarArgs) -> i32 {
    debug!("emscripten::___syscall196 (lstat64) {}", _which);
    let path_ptr: c_int = varargs.get(ctx);
    let buf_ptr: c_int = varargs.get(ctx);
    let path = emscripten_memory_pointer!(ctx.memory(0), path_ptr) as *const libc::c_char;
    let buf = emscripten_memory_pointer!(ctx.memory(0), buf_ptr) as *mut libc::c_void;
    let result = unsafe { lstat64(path, buf as _) };
    debug!(
        "=> path: {}, buf: {} = fd: {}\npath: {}\nlast os error: {}",
        path_ptr,
        buf_ptr,
        result,
        unsafe { std::ffi::CStr::from_ptr(path).to_str().unwrap() },
        Error::last_os_error(),
    );
    result
}

/// lchown
pub fn ___syscall198(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall198 (lchown) {}", _which);
    let path: c_int = varargs.get(ctx);
    let uid: libc::uid_t = varargs.get(ctx);
    let gid: libc::gid_t = varargs.get(ctx);
    let path_ptr = emscripten_memory_pointer!(ctx.memory(0), path) as *const i8;
    let result = unsafe { libc::lchown(path_ptr, uid, gid) };
    debug!(
        "=> path: {}, uid: {}, gid: {}, result: {}",
        unsafe { std::ffi::CStr::from_ptr(path_ptr).to_str().unwrap() },
        uid,
        gid,
        result,
    );
    result
}

/// getgid
pub fn ___syscall200(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall200 (getgid)");
    unsafe { libc::getgid() as i32 }
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

/// fchown
pub fn ___syscall207(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall207 (fchown) {}", _which);
    let fd: c_int = varargs.get(ctx);
    let owner: libc::uid_t = varargs.get(ctx);
    let group: libc::gid_t = varargs.get(ctx);
    unsafe { libc::fchown(fd, owner, group) }
}

/// getgroups
pub fn ___syscall205(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall205 (getgroups) {}", _which);
    let ngroups_max: c_int = varargs.get(ctx);
    let groups: c_int = varargs.get(ctx);

    #[allow(clippy::cast_ptr_alignment)]
    let gid_ptr = emscripten_memory_pointer!(ctx.memory(0), groups) as *mut libc::gid_t;
    assert_eq!(4, std::mem::align_of_val(&gid_ptr));
    let result = unsafe { libc::getgroups(ngroups_max, gid_ptr) };
    debug!(
        "=> ngroups_max: {}, gid_ptr: {:?}, result: {}",
        ngroups_max, gid_ptr, result,
    );
    result
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

/// madvise
pub fn ___syscall219(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall212 (madvise) {}", _which);

    let addr_ptr: c_int = varargs.get(ctx);
    let len: usize = varargs.get(ctx);
    let advice: c_int = varargs.get(ctx);

    let addr = emscripten_memory_pointer!(ctx.memory(0), addr_ptr) as *mut libc::c_void;

    unsafe { madvise(addr, len, advice) }
}

/// fallocate
pub fn ___syscall324(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall324 (fallocate) {}", _which);
    let _fd: c_int = varargs.get(ctx);
    let _mode: c_int = varargs.get(ctx);
    let _offset: libc::off_t = varargs.get(ctx);
    let _len: libc::off_t = varargs.get(ctx);
    #[cfg(not(target_os = "macos"))]
    unsafe {
        fallocate(_fd, _mode, _offset, _len)
    }
    #[cfg(target_os = "macos")]
    {
        unimplemented!()
    }
}
