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

/// read
pub fn ___syscall3(ctx: &mut Ctx, _which: i32, mut varargs: VarArgs) -> i32 {
    debug!("emscripten::___syscall3 (read) {}", _which);
    let fd: i32 = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);
    let count: i32 = varargs.get(ctx);
    debug!("=> fd: {}, buf_offset: {}, count: {}", fd, buf, count);
    let buf_addr = emscripten_memory_pointer!(ctx.memory(0), buf) as *mut libc::c_void;
    let ret = unsafe { libc::read(fd, buf_addr, count as _) };
    debug!("=> ret: {}", ret);
    ret as _
}

/// write
pub fn ___syscall4(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall4 (write) {}", _which);
    let fd: i32 = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);
    let count: i32 = varargs.get(ctx);
    debug!("=> fd: {}, buf: {}, count: {}", fd, buf, count);
    let buf_addr = emscripten_memory_pointer!(ctx.memory(0), buf) as *const libc::c_void;
    let ret = unsafe { libc::write(fd, buf_addr, count as _) as i32 };
    ret
}

/// open
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

/// close
pub fn ___syscall6(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall6 (close) {}", _which);
    let fd: i32 = varargs.get(ctx);
    debug!("fd: {}", fd);
    unsafe { libc::close(fd) }
}

/// link
pub fn ___syscall9(_ctx: &mut Ctx, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall9 (link) {}", _which);
    unimplemented!()
}

/// chmod
pub fn ___syscall15(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall15");
    -1
}

/// dup2
pub fn ___syscall63(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall63 (dup2) {}", _which);

    let src: i32 = varargs.get(ctx);
    let dst: i32 = varargs.get(ctx);

    unsafe { libc::dup2(src, dst) }
}

// writev
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall146(ctx: &mut Ctx, _which: i32, mut varargs: VarArgs) -> i32 {
    // -> ssize_t
    debug!("emscripten::___syscall146 (writev) {}", _which);
    let fd: i32 = varargs.get(ctx);
    let iov: i32 = varargs.get(ctx);
    let iovcnt: i32 = varargs.get(ctx);

    #[repr(C)]
    struct GuestIovec {
        iov_base: i32,
        iov_len: i32,
    }

    debug!("=> fd: {}, iov: {}, iovcnt = {}", fd, iov, iovcnt);
    let mut ret = 0;
    unsafe {
        for i in 0..iovcnt {
            let guest_iov_addr =
                emscripten_memory_pointer!(ctx.memory(0), (iov + i * 8)) as *mut GuestIovec;
            let iov_base = emscripten_memory_pointer!(ctx.memory(0), (*guest_iov_addr).iov_base)
                as *const libc::c_void;
            let iov_len = (*guest_iov_addr).iov_len as _;
            // debug!("=> iov_addr: {:?}, {:?}", iov_base, iov_len);
            let curr = libc::write(fd, iov_base, iov_len);
            if curr < 0 {
                return -1;
            }
            ret += curr;
        }
        // debug!(" => ret: {}", ret);
        ret as _
    }
}

/// ftruncate64
pub fn ___syscall194(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall194 - stub");
    unimplemented!()
}

// chown
pub fn ___syscall212(_ctx: &mut Ctx, which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall212 (chown) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    -1
}

/// access
pub fn ___syscall33(_ctx: &mut Ctx, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall33 (access) {}", _which);
    unimplemented!()
}

/// nice
pub fn ___syscall34(_ctx: &mut Ctx, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall34 (nice) {}", _which);
    unimplemented!()
}

// mkdir
pub fn ___syscall39(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall39 (mkdir) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    let pathname: u32 = varargs.get(ctx);
    let pathname_addr = emscripten_memory_pointer!(ctx.memory(0), pathname) as *const i8;
    unsafe { mkdir(pathname_addr) }
}

/// dup
pub fn ___syscall41(_ctx: &mut Ctx, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall41 (dup) {}", _which);
    unimplemented!()
}

/// getrusage
pub fn ___syscall77(_ctx: &mut Ctx, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall77 (getrusage) {}", _which);
    unimplemented!()
}

/// symlink
pub fn ___syscall83(_ctx: &mut Ctx, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall83 (symlink) {}", _which);
    unimplemented!()
}

/// lchown
pub fn ___syscall198(_ctx: &mut Ctx, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall198 (lchown) {}", _which);
    unimplemented!()
}

/// getgid
pub fn ___syscall200(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall200 (getgid)");
    unimplemented!()
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

/// getgroups
pub fn ___syscall205(_ctx: &mut Ctx, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall205 (getgroups) {}", _which);
    unimplemented!()
}

/// madvise
pub fn ___syscall219(_ctx: &mut Ctx, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall212 (chown) {}", _which);
    unimplemented!()
}

/// dup3
pub fn ___syscall330(_ctx: &mut Ctx, _which: c_int, mut _varargs: VarArgs) -> pid_t {
    debug!("emscripten::___syscall330 (dup3)");
    -1
}

/// pipe
pub fn ___syscall42(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall42 (pipe)");
    // offset to a file descriptor, which contains a read end and write end, 2 integers
    let fd_offset: u32 = varargs.get(ctx);

    let emscripten_memory = ctx.memory(0);

    // convert the file descriptor into a vec with two slots
    let mut fd_vec: Vec<c_int> = emscripten_memory.view()[((fd_offset / 4) as usize)..]
        .iter()
        .map(|pipe_end: &std::cell::Cell<c_int>| pipe_end.get())
        .take(2)
        .collect();

    // get it as a mutable pointer
    let fd_ptr = fd_vec.as_mut_ptr();

    // call pipe and store the pointers in this array
    #[cfg(target_os = "windows")]
    let result: c_int = unsafe { libc::pipe(fd_ptr, 2048, 0) };
    #[cfg(not(target_os = "windows"))]
    let result: c_int = unsafe { libc::pipe(fd_ptr) };
    result
}

/// ioctl
pub fn ___syscall54(_ctx: &mut Ctx, which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall54 (ioctl) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    -1
}

/// fchmod
pub fn ___syscall94(_ctx: &mut Ctx, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall118 (fchmod) {}", _which);
    unimplemented!()
}

// socketcall
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall102(_ctx: &mut Ctx, which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall102 (socketcall) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    -1
}

/// fsync
pub fn ___syscall118(_ctx: &mut Ctx, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall118 (fsync) {}", _which);
    unimplemented!()
}

// pread
pub fn ___syscall180(_ctx: &mut Ctx, which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall180 (pread) {}", which);
    #[cfg(not(feature = "debug"))]
    let _ = which;
    -1
}

// pwrite
pub fn ___syscall181(_ctx: &mut Ctx, which: c_int, mut _varargs: VarArgs) -> c_int {
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

/// fdatasync
pub fn ___syscall148(_ctx: &mut Ctx, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall148 (fdatasync) {}", _which);
    unimplemented!();
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

/// lstat64
pub fn ___syscall196(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall196 (lstat64) - stub");
    -1
}

/// fchown
pub fn ___syscall207(_ctx: &mut Ctx, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall207 (fchown) {}", _which);
    unimplemented!()
}

/// fallocate
pub fn ___syscall324(_ctx: &mut Ctx, _which: c_int, _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall324 (fallocate) {}", _which);
    unimplemented!()
}
