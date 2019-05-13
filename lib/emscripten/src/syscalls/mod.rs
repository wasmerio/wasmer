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
    off_t,
    //    open,
    read,
    rename,
    // sockaddr_in,
    // readv,
    rmdir,
    // writev,
    stat,
    write,
};
use wasmer_runtime_core::vm::Ctx;

use super::env;
use std::cell::Cell;
#[allow(unused_imports)]
use std::io::Error;
use std::mem;
use std::slice;

/// exit
pub fn ___syscall1(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) {
    debug!("emscripten::___syscall1 (exit) {}", _which);
    let status: i32 = varargs.get(ctx);
    unsafe {
        exit(status);
    }
}

/// read
pub fn ___syscall3(ctx: &mut Ctx, _which: i32, mut varargs: VarArgs) -> i32 {
    // -> ssize_t
    debug!("emscripten::___syscall3 (read) {}", _which);
    let fd: i32 = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);
    let count: i32 = varargs.get(ctx);
    debug!("=> fd: {}, buf_offset: {}, count: {}", fd, buf, count);
    let buf_addr = emscripten_memory_pointer!(ctx.memory(0), buf) as *mut c_void;
    let ret = unsafe { read(fd, buf_addr, count as _) };
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
    let buf_addr = emscripten_memory_pointer!(ctx.memory(0), buf) as *const c_void;
    unsafe { write(fd, buf_addr, count as _) as i32 }
}

/// close
pub fn ___syscall6(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall6 (close) {}", _which);
    let fd: i32 = varargs.get(ctx);
    debug!("fd: {}", fd);
    unsafe { close(fd) }
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

pub fn ___syscall15(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall15");
    -1
}

// getpid
pub fn ___syscall20(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall20 (getpid)");
    unsafe { getpid() }
}

// rename
pub fn ___syscall38(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> i32 {
    debug!("emscripten::___syscall38 (rename)");
    let old_path_addr: u32 = varargs.get(ctx);
    let new_path_addr: u32 = varargs.get(ctx);
    let old_path = emscripten_memory_pointer!(ctx.memory(0), old_path_addr) as *const i8;
    let new_path = emscripten_memory_pointer!(ctx.memory(0), new_path_addr) as *const i8;
    let result = unsafe { rename(old_path, new_path) };
    debug!(
        "=> old_path: {}, new_path: {}, result: {}",
        unsafe { std::ffi::CStr::from_ptr(old_path).to_str().unwrap() },
        unsafe { std::ffi::CStr::from_ptr(new_path).to_str().unwrap() },
        result
    );
    result
}

// rmdir
pub fn ___syscall40(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall40 (rmdir)");
    let pathname: u32 = varargs.get(ctx);
    let pathname_addr = emscripten_memory_pointer!(ctx.memory(0), pathname) as *const i8;
    unsafe { rmdir(pathname_addr) }
}

// pipe
pub fn ___syscall42(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall42 (pipe)");
    // offset to a file descriptor, which contains a read end and write end, 2 integers
    let fd_offset: u32 = varargs.get(ctx);

    let emscripten_memory = ctx.memory(0);

    // convert the file descriptor into a vec with two slots
    let mut fd_vec: Vec<c_int> = emscripten_memory.view()[((fd_offset / 4) as usize)..]
        .iter()
        .map(|pipe_end: &Cell<c_int>| pipe_end.get())
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

pub fn ___syscall60(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall60");
    -1
}

// dup2
pub fn ___syscall63(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall63 (dup2) {}", _which);

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
    debug!("emscripten::___syscall91 - stub");
    0
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
pub fn ___syscall183(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> i32 {
    debug!("emscripten::___syscall183");
    use std::env;
    let buf_offset: c_int = varargs.get(ctx);
    let _size: c_int = varargs.get(ctx);
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
            // ENOMEM
            return -12;
        }
        let real_ptr = emscripten_memory_pointer!(ctx.memory(0), ptr) as *const u8;
        env::call_memset(ctx, ptr, 0, len);
        for i in 0..(len as usize) {
            unsafe {
                assert_eq!(*real_ptr.add(i), 0);
            }
        }
        debug!("=> ptr: {}", ptr);
        return ptr as i32;
    } else {
        // return ENODEV
        return -19;
    }
}

/// lseek
pub fn ___syscall140(ctx: &mut Ctx, _which: i32, mut varargs: VarArgs) -> i32 {
    // -> c_int
    debug!("emscripten::___syscall140 (lseek) {}", _which);
    let fd: i32 = varargs.get(ctx);
    let _ = varargs.get::<i32>(ctx); // ignore high offset
    let offset_low: i32 = varargs.get(ctx);
    let result_ptr_value = varargs.get::<i32>(ctx);
    let whence: i32 = varargs.get(ctx);
    let offset = offset_low as off_t;
    let ret = unsafe { lseek(fd, offset, whence) as i32 };
    #[allow(clippy::cast_ptr_alignment)]
    let result_ptr = emscripten_memory_pointer!(ctx.memory(0), result_ptr_value) as *mut i32;
    assert_eq!(8, mem::align_of_val(&result_ptr));
    unsafe {
        *result_ptr = ret;
    }
    debug!(
        "=> fd: {}, offset: {}, result_ptr: {}, whence: {} = {}\nlast os error: {}",
        fd,
        offset,
        result_ptr_value,
        whence,
        0,
        Error::last_os_error(),
    );
    0
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

pub fn ___syscall199(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall199 - stub");
    -1
}

// stat64
pub fn ___syscall195(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall195 (stat64) {}", _which);
    let pathname: u32 = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);

    let pathname_addr = emscripten_memory_pointer!(ctx.memory(0), pathname) as *const i8;

    unsafe {
        let mut _stat: stat = std::mem::zeroed();
        let ret = stat(pathname_addr, &mut _stat);
        debug!(
            "=> pathname: {}, buf: {}, path: {} = {}\nlast os error: {}",
            pathname,
            buf,
            std::ffi::CStr::from_ptr(pathname_addr).to_str().unwrap(),
            ret,
            Error::last_os_error()
        );
        if ret != 0 {
            return ret;
        }
        copy_stat_into_wasm(ctx, buf, &_stat);
    }
    0
}

// fstat64
pub fn ___syscall197(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall197 (fstat64) {}", _which);
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
pub fn ___syscall221(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall221 (fcntl64) {}", _which);
    // fcntl64
    let _fd: i32 = varargs.get(ctx);
    let cmd: u32 = varargs.get(ctx);
    // (FAPPEND   - 0x08
    // |FASYNC    - 0x40
    // |FFSYNC    - 0x80
    // |FNONBLOCK - 0x04
    debug!("=> fd: {}, cmd: {}", _fd, cmd);
    match cmd {
        2 => 0,
        13 | 14 => 0, // pretend file locking worked
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

// utimensat
pub fn ___syscall320(_ctx: &mut Ctx, _which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall320 (utimensat), {}", _which);
    0
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
