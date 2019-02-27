#[cfg(unix)]
mod unix;

#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use self::unix::*;

#[cfg(windows)]
pub use self::windows::*;

use super::utils::copy_stat_into_wasm;
use super::varargs::VarArgs;
use byteorder::{ByteOrder, LittleEndian};
/// NOTE: TODO: These syscalls only support wasm_32 for now because they assume offsets are u32
/// Syscall list: https://www.cs.utexas.edu/~bismith/test/syscalls/syscalls32.html
use libc::{
    // ENOTTY,
    c_int,
    c_void,
    chdir,
    // fcntl, setsockopt, getppid
    close,
    dup2,
    exit,
    fstat,
    getpid,
    // iovec,
    lseek,
    //    open,
    read,
    // readv,
    rmdir,
    // writev,
    stat,
    write,
    // sockaddr_in,
};
use wasmer_runtime_core::vm::Ctx;

use super::env;
use std::slice;
// use std::sys::fd::FileDesc;

// Another conditional constant for name resolution: Macos et iOS use
// SO_NOSIGPIPE as a setsockopt flag to disable SIGPIPE emission on socket.
// Other platforms do otherwise.
use crate::env::get_emscripten_data;
use crate::utils::copy_cstr_into_wasm;
use crate::utils::read_string_from_wasm;
#[cfg(target_os = "darwin")]
use libc::SO_NOSIGPIPE;
use std::ffi::CString;

#[cfg(not(target_os = "darwin"))]
const SO_NOSIGPIPE: c_int = 0;

/// exit
pub fn ___syscall1(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) {
    debug!("emscripten::___syscall1 (exit) {}", which);
    let status: i32 = varargs.get(ctx);
    unsafe {
        exit(status);
    }
}

/// read
pub fn ___syscall3(ctx: &mut Ctx, which: i32, mut varargs: VarArgs) -> i32 {
    // -> ssize_t
    debug!("emscripten::___syscall3 (read) {}", which);
    let fd: i32 = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);
    let count = varargs.get(ctx);
    debug!("=> fd: {}, buf_offset: {}, count: {}", fd, buf, count);
    let buf_addr = emscripten_memory_pointer!(ctx.memory(0), buf) as *mut c_void;
    let ret = unsafe { read(fd, buf_addr, count) };
    debug!("=> ret: {}", ret);
    ret as _
}

/// write
pub fn ___syscall4(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall4 (write) {}", which);
    let fd: i32 = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);
    let count = varargs.get(ctx);
    debug!("=> fd: {}, buf: {}, count: {}", fd, buf, count);
    let buf_addr = emscripten_memory_pointer!(ctx.memory(0), buf) as *const c_void;
    unsafe { write(fd, buf_addr, count) as i32 }
}

/// close
pub fn ___syscall6(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall6 (close) {}", which);
    let fd: i32 = varargs.get(ctx);
    debug!("fd: {}", fd);
    unsafe { close(fd) }
}

// chdir
pub fn ___syscall12(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall12 (chdir) {}", which);
    let path_addr: i32 = varargs.get(ctx);
    unsafe {
        let path_ptr = emscripten_memory_pointer!(ctx.memory(0), path_addr) as *const i8;
        let path = std::ffi::CStr::from_ptr(path_ptr);
        let ret = chdir(path_ptr);
        debug!("=> path: {:?}, ret: {}", path, ret);
        ret
    }
}

pub fn ___syscall10(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall10");
    -1
}

pub fn ___syscall15(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall15");
    -1
}

// getpid
pub fn ___syscall20(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall20 (getpid)");
    unsafe { getpid() }
}

pub fn ___syscall38(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall38");
    -1
}

// rmdir
pub fn ___syscall40(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall40 (rmdir)");
    let pathname: u32 = varargs.get(ctx);
    let pathname_addr = emscripten_memory_pointer!(ctx.memory(0), pathname) as *const i8;
    unsafe { rmdir(pathname_addr) }
}

pub fn ___syscall60(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall60");
    -1
}

// dup2
pub fn ___syscall63(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall63 (dup2) {}", which);

    let src: i32 = varargs.get(ctx);
    let dst: i32 = varargs.get(ctx);

    unsafe { dup2(src, dst) }
}

// getppid
pub fn ___syscall64(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall64 (getppid)");
    unsafe { getpid() }
}

pub fn ___syscall66(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall66");
    -1
}

pub fn ___syscall75(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall75");
    -1
}

pub fn ___syscall85(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall85");
    -1
}

pub fn ___syscall91(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall91");
    -1
}

pub fn ___syscall97(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall97");
    -1
}

pub fn ___syscall110(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall110");
    -1
}

// getcwd
pub fn ___syscall183(ctx: &mut Ctx, buf_offset: u32, _size: u32) -> u32 {
    debug!("emscripten::___syscall183");
    use std::env;
    let path = env::current_dir();
    let path_string = path.unwrap().display().to_string();
    let len = path_string.len();
    unsafe {
        let pointer_to_buffer =
            emscripten_memory_pointer!(ctx.memory(0), buf_offset) as *mut libc::c_char;
        let slice = slice::from_raw_parts_mut(pointer_to_buffer, len.clone());
        for (byte, loc) in path_string.bytes().zip(slice.iter_mut()) {
            *loc = byte as _;
        }
        *pointer_to_buffer.add(len.clone()) = 0;
    }
    buf_offset
}

// mmap2
pub fn ___syscall192(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall192 (mmap2) {}", which);
    let addr: i32 = varargs.get(ctx);
    let len: u32 = varargs.get(ctx);
    let prot: i32 = varargs.get(ctx);
    let flags: i32 = varargs.get(ctx);
    let fd: i32 = varargs.get(ctx);
    let off: i32 = varargs.get(ctx);
    debug!(
        "=> addr: {}, len: {}, prot: {}, flags: {}, fd: {}, off: {}",
        addr, len, prot, flags, fd, off
    );

    if fd == -1 {
        let ptr = env::call_memalign(ctx, 16384, len);
        if ptr == 0 {
            return -1;
        }
        env::call_memset(ctx, ptr, 0, len);
        ptr as _
    } else {
        -1
    }
}

/// lseek
pub fn ___syscall140(ctx: &mut Ctx, which: i32, mut varargs: VarArgs) -> i32 {
    // -> c_int
    debug!("emscripten::___syscall140 (lseek) {}", which);
    let fd: i32 = varargs.get(ctx);
    let offset = varargs.get(ctx);
    let whence: i32 = varargs.get(ctx);
    debug!("=> fd: {}, offset: {}, whence = {}", fd, offset, whence);
    unsafe { lseek(fd, offset, whence) as _ }
}

/// readv
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall145(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> i32 {
    // -> ssize_t
    debug!("emscripten::___syscall145 (readv) {}", which);
    // let fd: i32 = varargs.get(ctx);
    // let iov: u32 = varargs.get(ctx);
    // let iovcnt: i32 = varargs.get(ctx);
    // debug!("=> fd: {}, iov: {}, iovcnt = {}", fd, iov, iovcnt);
    // let iov_addr = emscripten_memory_pointer!(ctx.memory(0), iov) as *mut iovec;
    // unsafe { readv(fd, iov_addr, iovcnt) }

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
                as *mut c_void;
            let iov_len = (*guest_iov_addr).iov_len as _;
            // debug!("=> iov_addr: {:?}, {:?}", iov_base, iov_len);
            let curr = read(fd, iov_base, iov_len);
            if curr < 0 {
                return -1;
            }
            ret += curr;
        }
        // debug!(" => ret: {}", ret);
        ret as _
    }
}

// writev
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall146(ctx: &mut Ctx, which: i32, mut varargs: VarArgs) -> i32 {
    // -> ssize_t
    debug!("emscripten::___syscall146 (writev) {}", which);
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
                as *const c_void;
            let iov_len = (*guest_iov_addr).iov_len as _;
            // debug!("=> iov_addr: {:?}, {:?}", iov_base, iov_len);
            let curr = write(fd, iov_base, iov_len);
            if curr < 0 {
                return -1;
            }
            ret += curr;
        }
        // debug!(" => ret: {}", ret);
        ret as _
    }
}

pub fn ___syscall168(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall168");
    -1
}

pub fn ___syscall191(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall191 - stub");
    -1
}

pub fn ___syscall194(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall194 - stub");
    -1
}

pub fn ___syscall196(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall194 - stub");
    -1
}

pub fn ___syscall199(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall199 - stub");
    -1
}

// stat64
pub fn ___syscall195(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall195 (stat64) {}", which);
    let pathname: u32 = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);

    let pathname_addr = emscripten_memory_pointer!(ctx.memory(0), pathname) as *const i8;

    unsafe {
        let mut _stat: stat = std::mem::zeroed();
        let ret = stat(pathname_addr, &mut _stat);
        debug!("ret: {}", ret);
        if ret != 0 {
            return ret;
        }
        copy_stat_into_wasm(ctx, buf, &_stat);
    }
    0
}

// fstat64
pub fn ___syscall197(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall197 (fstat64) {}", which);
    let fd: c_int = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);

    unsafe {
        let mut stat = std::mem::zeroed();
        let ret = fstat(fd, &mut stat);
        debug!("ret: {}", ret);
        if ret != 0 {
            return ret;
        }
        copy_stat_into_wasm(ctx, buf, &stat);
    }

    0
}

pub fn ___syscall220(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall220");
    -1
}

// fcntl64
pub fn ___syscall221(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall221 (fcntl64) {}", which);
    // fcntl64
    let _fd: i32 = varargs.get(ctx);
    let cmd: u32 = varargs.get(ctx);
    match cmd {
        2 => 0,
        _ => -1,
    }
}

pub fn ___syscall268(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall268");
    -1
}

pub fn ___syscall272(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall272");
    -1
}

pub fn ___syscall295(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall295");
    -1
}

pub fn ___syscall300(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall300");
    -1
}

pub fn ___syscall334(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall334");
    -1
}

// prlimit64
pub fn ___syscall340(ctx: &mut Ctx, which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall340 (prlimit64), {}", which);
    // NOTE: Doesn't really matter. Wasm modules cannot exceed WASM_PAGE_SIZE anyway.
    let _pid: i32 = varargs.get(ctx);
    let _resource: i32 = varargs.get(ctx);
    let _new_limit: u32 = varargs.get(ctx);
    let old_limit: u32 = varargs.get(ctx);

    if old_limit != 0 {
        // just report no limits
        let buf_ptr = emscripten_memory_pointer!(ctx.memory(0), old_limit) as *mut u8;
        let buf = unsafe { slice::from_raw_parts_mut(buf_ptr, 16) };

        LittleEndian::write_i32(&mut buf[..], -1); // RLIM_INFINITY
        LittleEndian::write_i32(&mut buf[4..], -1); // RLIM_INFINITY
        LittleEndian::write_i32(&mut buf[8..], -1); // RLIM_INFINITY
        LittleEndian::write_i32(&mut buf[12..], -1); // RLIM_INFINITY
    }

    0
}
