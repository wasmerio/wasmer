#[cfg(unix)]
mod unix;

#[cfg(windows)]
mod windows;

#[cfg(feature = "vfs")]
mod emscripten_vfs;

#[cfg(unix)]
pub use self::unix::*;

#[cfg(windows)]
pub use self::windows::*;

#[cfg(feature = "vfs")]
pub use self::emscripten_vfs::*;

use super::varargs::VarArgs;
use byteorder::{ByteOrder, LittleEndian};
/// NOTE: TODO: These syscalls only support wasm_32 for now because they assume offsets are u32
/// Syscall list: https://www.cs.utexas.edu/~bismith/test/syscalls/syscalls32.html
use libc::{c_int, c_void, chdir, exit, getpid, lseek, rmdir, write};
use wasmer_runtime_core::vm::Ctx;

use super::env;
use std::slice;

/// exit
pub fn ___syscall1(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) {
    debug!("emscripten::___syscall1 (exit) {}", _which);
    let status: i32 = varargs.get(ctx);
    unsafe {
        exit(status);
    }
}

// chdir
pub fn ___syscall12(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall12 (chdir) {}", _which);
    let path_addr: i32 = varargs.get(ctx);
    unsafe {
        let path_ptr = emscripten_memory_pointer!(ctx.memory(0), path_addr) as *const i8;
        let _path = std::ffi::CStr::from_ptr(path_ptr);
        let ret = chdir(path_ptr);
        debug!("=> path: {:?}, ret: {}", _path, ret);
        ret
    }
}

pub fn ___syscall10(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall10");
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

// getppid
pub fn ___syscall64(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall64 (getppid)");
    let result = unsafe { getpid() };
    result
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
pub fn ___syscall192(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall192 (mmap2) {}", _which);
    let _addr: i32 = varargs.get(ctx);
    let len: u32 = varargs.get(ctx);
    let _prot: i32 = varargs.get(ctx);
    let _flags: i32 = varargs.get(ctx);
    let fd: i32 = varargs.get(ctx);
    let _off: i32 = varargs.get(ctx);
    debug!(
        "=> addr: {}, len: {}, prot: {}, flags: {}, fd: {}, off: {}",
        _addr, len, _prot, _flags, fd, _off
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
pub fn ___syscall140(ctx: &mut Ctx, _which: i32, mut varargs: VarArgs) -> i32 {
    // -> c_int
    debug!("emscripten::___syscall140 (lseek) {}", _which);
    let fd: i32 = varargs.get(ctx);
    let offset: i32 = varargs.get(ctx);
    let whence: i32 = varargs.get(ctx);
    debug!("=> fd: {}, offset: {}, whence = {}", fd, offset, whence);
    unsafe { lseek(fd, offset as _, whence) as _ }
}

/// readv
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall145(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> i32 {
    // -> ssize_t
    debug!("emscripten::___syscall145 (readv) {}", _which);

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
            let curr = libc::read(fd, iov_base, iov_len);
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

pub fn ___syscall220(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall220");
    -1
}

// fcntl64
pub fn ___syscall221(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall221 (fcntl64) {}", _which);
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
pub fn ___syscall340(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall340 (prlimit64), {}", _which);
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
