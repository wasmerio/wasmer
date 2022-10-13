#[cfg(unix)]
mod unix;

#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use self::unix::*;

#[cfg(windows)]
pub use self::windows::*;

use crate::{
    utils::{copy_stat_into_wasm, get_cstr_path, get_current_directory},
    EmEnv,
};

use super::varargs::VarArgs;
use byteorder::{ByteOrder, LittleEndian};
/// NOTE: TODO: These syscalls only support wasm_32 for now because they assume offsets are u32
/// Syscall list: https://www.cs.utexas.edu/~bismith/test/syscalls/syscalls32.html
use libc::{
    c_int,
    c_void,
    chdir,
    // setsockopt, getppid
    close,
    dup2,
    exit,
    fstat,
    getpid,
    // readlink,
    // iovec,
    lseek,
    //    open,
    read,
    rename,
    // sockaddr_in,
    // readv,
    rmdir,
    // writev,
    stat,
    write,
    // ENOTTY,
};

use super::env;
#[allow(unused_imports)]
use std::io::Error;
use std::slice;
use wasmer::{FunctionEnvMut, WasmPtr};

/// exit
pub fn ___syscall1(ctx: FunctionEnvMut<EmEnv>, _which: c_int, mut varargs: VarArgs) {
    debug!("emscripten::___syscall1 (exit) {}", _which);
    let status: i32 = varargs.get(&ctx);
    unsafe {
        exit(status);
    }
}

/// read
pub fn ___syscall3(ctx: FunctionEnvMut<EmEnv>, _which: i32, mut varargs: VarArgs) -> i32 {
    // -> ssize_t
    debug!("emscripten::___syscall3 (read) {}", _which);
    let fd: i32 = varargs.get(&ctx);
    let buf: u32 = varargs.get(&ctx);
    let count: i32 = varargs.get(&ctx);
    debug!("=> fd: {}, buf_offset: {}, count: {}", fd, buf, count);
    let memory = ctx.data().memory(0);
    let buf_addr = emscripten_memory_pointer!(memory.view(&ctx), buf) as *mut c_void;
    let ret = unsafe { read(fd, buf_addr, count as _) };
    debug!("=> ret: {}", ret);
    ret as _
}

/// write
pub fn ___syscall4(ctx: FunctionEnvMut<EmEnv>, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall4 (write) {}", _which);
    let fd: i32 = varargs.get(&ctx);
    let buf: i32 = varargs.get(&ctx);
    let count: i32 = varargs.get(&ctx);
    debug!("=> fd: {}, buf: {}, count: {}", fd, buf, count);
    let memory = ctx.data().memory(0);
    let buf_addr = emscripten_memory_pointer!(memory.view(&ctx), buf) as *const c_void;
    unsafe { write(fd, buf_addr, count as _) as i32 }
}

/// close
pub fn ___syscall6(ctx: FunctionEnvMut<EmEnv>, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall6 (close) {}", _which);
    let fd: i32 = varargs.get(&ctx);
    debug!("fd: {}", fd);
    unsafe { close(fd) }
}

// chdir
pub fn ___syscall12(ctx: FunctionEnvMut<EmEnv>, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall12 (chdir) {}", _which);
    let path_ptr = varargs.get_str(&ctx);
    let real_path_owned = get_cstr_path(ctx, path_ptr as *const _);
    let real_path = if let Some(ref rp) = real_path_owned {
        rp.as_c_str().as_ptr()
    } else {
        path_ptr
    };
    let ret = unsafe { chdir(real_path) };
    debug!(
        "=> path: {:?}, ret: {}",
        unsafe { std::ffi::CStr::from_ptr(real_path) },
        ret
    );
    ret
}

pub fn ___syscall10(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall10");
    -1
}

pub fn ___syscall14(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall14");
    -1
}

pub fn ___syscall15(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall15");
    -1
}

// getpid
pub fn ___syscall20(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall20 (getpid)");
    unsafe { getpid() }
}

pub fn ___syscall21(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall21");
    -1
}

pub fn ___syscall25(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall25");
    -1
}

pub fn ___syscall29(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall29");
    -1
}

pub fn ___syscall32(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall32");
    -1
}

pub fn ___syscall33(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall33");
    -1
}

pub fn ___syscall36(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall36");
    -1
}

// rename
pub fn ___syscall38(mut ctx: FunctionEnvMut<EmEnv>, _which: c_int, mut varargs: VarArgs) -> i32 {
    debug!("emscripten::___syscall38 (rename)");
    let old_path = varargs.get_str(&ctx);
    let new_path = varargs.get_str(&ctx);
    let real_old_path_owned = get_cstr_path(ctx.as_mut(), old_path as *const _);
    let real_old_path = if let Some(ref rp) = real_old_path_owned {
        rp.as_c_str().as_ptr()
    } else {
        old_path
    };
    let real_new_path_owned = get_cstr_path(ctx, new_path as *const _);
    let real_new_path = if let Some(ref rp) = real_new_path_owned {
        rp.as_c_str().as_ptr()
    } else {
        new_path
    };
    let result = unsafe { rename(real_old_path, real_new_path) };
    debug!(
        "=> old_path: {}, new_path: {}, result: {}",
        unsafe { std::ffi::CStr::from_ptr(real_old_path).to_str().unwrap() },
        unsafe { std::ffi::CStr::from_ptr(real_new_path).to_str().unwrap() },
        result
    );
    result
}

// rmdir
pub fn ___syscall40(ctx: FunctionEnvMut<EmEnv>, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall40 (rmdir)");
    let pathname_addr = varargs.get_str(&ctx);
    let real_path_owned = get_cstr_path(ctx, pathname_addr as *const _);
    let real_path = if let Some(ref rp) = real_path_owned {
        rp.as_c_str().as_ptr()
    } else {
        pathname_addr
    };
    unsafe { rmdir(real_path) }
}

// pipe
pub fn ___syscall42(ctx: FunctionEnvMut<EmEnv>, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall42 (pipe)");
    // offset to a file descriptor, which contains a read end and write end, 2 integers
    let fd_offset: u32 = varargs.get(&ctx);

    let memory = ctx.data().memory(0);
    let emscripten_memory = memory.view(&ctx);

    // convert the file descriptor into a vec with two slots
    let mut fd_vec: [c_int; 2] = WasmPtr::<[c_int; 2]>::new(fd_offset)
        .deref(&emscripten_memory)
        .read()
        .unwrap();

    // get it as a mutable pointer
    let fd_ptr = fd_vec.as_mut_ptr();

    // call pipe and store the pointers in this array
    #[cfg(target_os = "windows")]
    let result: c_int = unsafe { libc::pipe(fd_ptr, 2048, 0) };
    #[cfg(not(target_os = "windows"))]
    let result: c_int = unsafe { libc::pipe(fd_ptr) };
    if result == -1 {
        debug!("=> os error: {}", Error::last_os_error());
    }
    result
}

pub fn ___syscall51(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall51");
    -1
}

pub fn ___syscall52(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall52");
    -1
}

pub fn ___syscall53(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall53");
    -1
}

pub fn ___syscall60(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall60");
    -1
}

// dup2
pub fn ___syscall63(ctx: FunctionEnvMut<EmEnv>, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall63 (dup2) {}", _which);

    let src: i32 = varargs.get(&ctx);
    let dst: i32 = varargs.get(&ctx);

    unsafe { dup2(src, dst) }
}

// getppid
pub fn ___syscall64(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall64 (getppid)");
    unsafe { getpid() }
}

pub fn ___syscall66(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall66");
    -1
}

pub fn ___syscall75(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall75");
    -1
}

pub fn ___syscall91(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall91 - stub");
    0
}

pub fn ___syscall96(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall96");
    -1
}

pub fn ___syscall97(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall97");
    -1
}

pub fn ___syscall110(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall110");
    -1
}

pub fn ___syscall121(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall121");
    -1
}

pub fn ___syscall125(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall125");
    -1
}

pub fn ___syscall133(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall133");
    -1
}

pub fn ___syscall144(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall144");
    -1
}

pub fn ___syscall147(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall147");
    -1
}

pub fn ___syscall150(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall150");
    -1
}

pub fn ___syscall151(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall151");
    -1
}

pub fn ___syscall152(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall152");
    -1
}

pub fn ___syscall153(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall153");
    -1
}

pub fn ___syscall163(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall163");
    -1
}

// getcwd
pub fn ___syscall183(mut ctx: FunctionEnvMut<EmEnv>, _which: c_int, mut varargs: VarArgs) -> i32 {
    debug!("emscripten::___syscall183");
    let buf_offset: WasmPtr<libc::c_char> = varargs.get(&ctx);
    let _size: c_int = varargs.get(&ctx);
    let path = get_current_directory(ctx.as_mut());
    let path_string = path.unwrap().display().to_string();
    let len = path_string.len();
    let memory = ctx.data().memory(0);
    let memory = memory.view(&ctx);

    let buf_writer = buf_offset.slice(&memory, len as u32 + 1).unwrap();
    for (i, byte) in path_string.bytes().enumerate() {
        buf_writer.index(i as u64).write(byte as _).unwrap();
    }
    buf_writer.index(len as u64).write(0).unwrap();
    buf_offset.offset() as i32
}

// mmap2
pub fn ___syscall192(mut ctx: FunctionEnvMut<EmEnv>, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall192 (mmap2) {}", _which);
    let _addr: i32 = varargs.get(&ctx);
    let len: u32 = varargs.get(&ctx);
    let _prot: i32 = varargs.get(&ctx);
    let _flags: i32 = varargs.get(&ctx);
    let fd: i32 = varargs.get(&ctx);
    let _off: i32 = varargs.get(&ctx);
    debug!(
        "=> addr: {}, len: {}, prot: {}, flags: {}, fd: {}, off: {}",
        _addr, len, _prot, _flags, fd, _off
    );

    if fd == -1 {
        let ptr = env::call_memalign(&mut ctx, 16384, len);
        if ptr == 0 {
            // ENOMEM
            return -12;
        }
        let memory = ctx.data().memory(0);
        let real_ptr = emscripten_memory_pointer!(memory.view(&ctx), ptr) as *const u8;
        env::call_memset(&mut ctx, ptr, 0, len);
        for i in 0..(len as usize) {
            unsafe {
                assert_eq!(*real_ptr.add(i), 0);
            }
        }
        debug!("=> ptr: {}", ptr);
        ptr as i32
    } else {
        // return ENODEV
        -19
    }
}

/// lseek
pub fn ___syscall140(ctx: FunctionEnvMut<EmEnv>, _which: i32, mut varargs: VarArgs) -> i32 {
    // -> c_int
    debug!("emscripten::___syscall140 (lseek) {}", _which);
    let fd: i32 = varargs.get(&ctx);
    let _offset_high: u32 = varargs.get(&ctx); // We don't use the offset high as emscripten skips it
    let offset_low: u32 = varargs.get(&ctx);
    let result_ptr_value: WasmPtr<i64> = varargs.get(&ctx);
    let whence: i32 = varargs.get(&ctx);
    let offset = offset_low;
    let ret = unsafe { lseek(fd, offset as _, whence) as i64 };
    let memory = ctx.data().memory(0);
    let memory = memory.view(&ctx);

    let result_ptr = result_ptr_value.deref(&memory);
    result_ptr.write(ret).unwrap();

    debug!(
        "=> fd: {}, offset: {}, result: {}, whence: {} = {}\nlast os error: {}",
        fd,
        offset,
        ret,
        whence,
        0,
        Error::last_os_error(),
    );
    0
}

/// readv
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall145(ctx: FunctionEnvMut<EmEnv>, _which: c_int, mut varargs: VarArgs) -> i32 {
    // -> ssize_t
    debug!("emscripten::___syscall145 (readv) {}", _which);

    let fd: i32 = varargs.get(&ctx);
    let iov: i32 = varargs.get(&ctx);
    let iovcnt: i32 = varargs.get(&ctx);

    #[repr(C)]
    struct GuestIovec {
        iov_base: i32,
        iov_len: i32,
    }

    debug!("=> fd: {}, iov: {}, iovcnt = {}", fd, iov, iovcnt);
    let mut ret = 0;
    unsafe {
        for i in 0..iovcnt {
            let memory = ctx.data().memory(0);
            let guest_iov_addr =
                emscripten_memory_pointer!(memory.view(&ctx), (iov + i * 8)) as *mut GuestIovec;
            let iov_base = emscripten_memory_pointer!(memory.view(&ctx), (*guest_iov_addr).iov_base)
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
pub fn ___syscall146(ctx: FunctionEnvMut<EmEnv>, _which: i32, mut varargs: VarArgs) -> i32 {
    // -> ssize_t
    debug!("emscripten::___syscall146 (writev) {}", _which);
    let fd: i32 = varargs.get(&ctx);
    let iov: i32 = varargs.get(&ctx);
    let iovcnt: i32 = varargs.get(&ctx);

    #[repr(C)]
    struct GuestIovec {
        iov_base: i32,
        iov_len: i32,
    }

    debug!("=> fd: {}, iov: {}, iovcnt = {}", fd, iov, iovcnt);
    let mut ret = 0;
    for i in 0..iovcnt {
        unsafe {
            let memory = ctx.data().memory(0);
            let guest_iov_addr =
                emscripten_memory_pointer!(memory.view(&ctx), (iov + i * 8)) as *mut GuestIovec;
            let iov_base = emscripten_memory_pointer!(memory.view(&ctx), (*guest_iov_addr).iov_base)
                as *const c_void;
            let iov_len = (*guest_iov_addr).iov_len as _;
            // debug!("=> iov_addr: {:?}, {:?}", iov_base, iov_len);
            let curr = write(fd, iov_base, iov_len);
            debug!(
                "=> iov_base: {}, iov_len: {}, curr = {}",
                (*guest_iov_addr).iov_base,
                iov_len,
                curr
            );
            if curr < 0 {
                debug!("=> os error: {}", Error::last_os_error());
                return -1;
            }
            ret += curr;
        }
    }
    debug!(" => ret: {}", ret);
    ret as _
}

pub fn ___syscall191(ctx: FunctionEnvMut<EmEnv>, _which: i32, mut varargs: VarArgs) -> i32 {
    let _resource: i32 = varargs.get(&ctx);
    debug!(
        "emscripten::___syscall191 - mostly stub, resource: {}",
        _resource
    );
    let rlim_emptr: i32 = varargs.get(&ctx);
    let memory = ctx.data().memory(0);
    let rlim_ptr = emscripten_memory_pointer!(memory.view(&ctx), rlim_emptr) as *mut u8;
    let rlim = unsafe { slice::from_raw_parts_mut(rlim_ptr, 16) };

    // set all to RLIM_INIFINTY
    LittleEndian::write_i64(&mut *rlim, -1);
    LittleEndian::write_i64(&mut rlim[8..], -1);

    0
}

pub fn ___syscall193(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall193");
    -1
}

// stat64
pub fn ___syscall195(mut ctx: FunctionEnvMut<EmEnv>, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall195 (stat64) {}", _which);
    let pathname_addr = varargs.get_str(&ctx);
    let buf: u32 = varargs.get(&ctx);

    let real_path_owned = get_cstr_path(ctx.as_mut(), pathname_addr as *const _);
    let real_path = if let Some(ref rp) = real_path_owned {
        rp.as_c_str().as_ptr()
    } else {
        pathname_addr
    };

    unsafe {
        let mut _stat: stat = std::mem::zeroed();
        let ret = stat(real_path, &mut _stat);
        debug!(
            "=> pathname: {}, buf: {} = {}",
            std::ffi::CStr::from_ptr(real_path).to_str().unwrap(),
            buf,
            ret
        );
        if ret != 0 {
            debug!("=> os error: {}", Error::last_os_error());
            return ret;
        }
        copy_stat_into_wasm(ctx, buf, &_stat);
    }
    0
}

// fstat64
pub fn ___syscall197(ctx: FunctionEnvMut<EmEnv>, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall197 (fstat64) {}", _which);

    let fd: c_int = varargs.get(&ctx);
    let buf: u32 = varargs.get(&ctx);

    unsafe {
        let mut stat = std::mem::zeroed();
        let ret = fstat(fd, &mut stat);
        debug!("=> fd: {}, buf: {} = {}", fd, buf, ret);
        if ret != 0 {
            debug!("=> os error: {}", Error::last_os_error());
            return ret;
        }
        copy_stat_into_wasm(ctx, buf, &stat);
    }
    0
}

pub fn ___syscall209(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall209");
    -1
}

pub fn ___syscall211(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall211");
    -1
}

pub fn ___syscall218(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall218");
    -1
}

pub fn ___syscall268(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall268");
    -1
}

pub fn ___syscall269(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall269");
    -1
}

pub fn ___syscall272(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall272");
    -1
}

pub fn ___syscall295(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall295");
    -1
}

pub fn ___syscall296(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall296");
    -1
}

pub fn ___syscall297(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall297");
    -1
}

pub fn ___syscall298(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall298");
    -1
}

pub fn ___syscall300(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall300");
    -1
}

pub fn ___syscall301(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall301");
    -1
}

pub fn ___syscall302(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall302");
    -1
}

pub fn ___syscall303(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall303");
    -1
}

pub fn ___syscall304(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall304");
    -1
}

pub fn ___syscall305(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall305");
    -1
}

pub fn ___syscall306(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall306");
    -1
}

pub fn ___syscall307(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall307");
    -1
}

pub fn ___syscall308(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall308");
    -1
}

// utimensat
pub fn ___syscall320(_ctx: FunctionEnvMut<EmEnv>, _which: c_int, mut _varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall320 (utimensat), {}", _which);
    0
}

pub fn ___syscall331(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall331");
    -1
}

pub fn ___syscall333(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall333");
    -1
}

pub fn ___syscall334(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall334");
    -1
}

pub fn ___syscall337(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall337");
    -1
}

// prlimit64
pub fn ___syscall340(ctx: FunctionEnvMut<EmEnv>, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall340 (prlimit64), {}", _which);
    // NOTE: Doesn't really matter. Wasm modules cannot exceed WASM_PAGE_SIZE anyway.
    let _pid: i32 = varargs.get(&ctx);
    let resource: i32 = varargs.get(&ctx);
    let _new_limit: u32 = varargs.get(&ctx);
    let old_limit: u32 = varargs.get(&ctx);

    let val = match resource {
        // RLIMIT_NOFILE
        7 => 1024,
        _ => -1, // RLIM_INFINITY
    };

    if old_limit != 0 {
        // just report no limits
        let memory = ctx.data().memory(0);
        let buf_ptr = emscripten_memory_pointer!(memory.view(&ctx), old_limit) as *mut u8;
        let buf = unsafe { slice::from_raw_parts_mut(buf_ptr, 16) };

        LittleEndian::write_i64(&mut *buf, val);
        LittleEndian::write_i64(&mut buf[8..], val);
    }

    0
}

pub fn ___syscall345(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall345");
    -1
}
