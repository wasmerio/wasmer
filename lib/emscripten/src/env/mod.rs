#[cfg(unix)]
mod unix;

#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use self::unix::*;

#[cfg(windows)]
pub use self::windows::*;

use libc::c_char;

use crate::{allocate_on_stack, EmscriptenData, EmscriptenFunctions};

use std::os::raw::c_int;
use std::sync::MutexGuard;

use crate::EmEnv;
use wasmer::ValueType;
use wasmer::{FunctionEnvMut, WasmPtr};

pub fn call_malloc(mut ctx: &mut FunctionEnvMut<EmEnv>, size: u32) -> u32 {
    let malloc_ref = get_emscripten_funcs(ctx).malloc_ref().unwrap().clone();
    malloc_ref.call(&mut ctx, size).unwrap()
}

#[allow(dead_code)]
pub fn call_malloc_with_cast<T: Copy>(ctx: &mut FunctionEnvMut<EmEnv>, size: u32) -> WasmPtr<T> {
    WasmPtr::new(call_malloc(ctx, size))
}

pub fn call_memalign(mut ctx: &mut FunctionEnvMut<EmEnv>, alignment: u32, size: u32) -> u32 {
    let memalign_ref = get_emscripten_funcs(ctx).memalign_ref().unwrap().clone();
    memalign_ref.call(&mut ctx, alignment, size).unwrap()
}

pub fn call_memset(
    mut ctx: &mut FunctionEnvMut<EmEnv>,
    pointer: u32,
    value: u32,
    size: u32,
) -> u32 {
    let memset_ref = get_emscripten_funcs(ctx).memset_ref().unwrap().clone();
    memset_ref.call(&mut ctx, pointer, value, size).unwrap()
}

pub(crate) fn get_emscripten_data<'a>(
    ctx: &'a FunctionEnvMut<EmEnv>,
) -> MutexGuard<'a, Option<EmscriptenData>> {
    ctx.data().data.lock().unwrap()
}

pub(crate) fn get_emscripten_funcs<'a>(
    ctx: &'a FunctionEnvMut<EmEnv>,
) -> MutexGuard<'a, EmscriptenFunctions> {
    ctx.data().funcs.lock().unwrap()
}

pub fn _getpagesize(_ctx: FunctionEnvMut<EmEnv>) -> u32 {
    debug!("emscripten::_getpagesize");
    16384
}

pub fn _times(mut ctx: FunctionEnvMut<EmEnv>, buffer: u32) -> u32 {
    if buffer != 0 {
        call_memset(&mut ctx, buffer, 0, 16);
    }
    0
}

#[allow(clippy::cast_ptr_alignment)]
pub fn ___build_environment(mut ctx: FunctionEnvMut<EmEnv>, environ: c_int) {
    debug!("emscripten::___build_environment {}", environ);
    const MAX_ENV_VALUES: u32 = 64;
    const TOTAL_ENV_SIZE: u32 = 1024;
    let memory = ctx.data().memory(0);
    let environment = emscripten_memory_pointer!(memory.view(&ctx), environ) as *mut c_int;
    let (mut pool_offset, env_ptr, mut pool_ptr) = unsafe {
        let (pool_offset, _pool_slice): (u32, &mut [u8]) =
            allocate_on_stack(&mut ctx, TOTAL_ENV_SIZE as u32);
        let (env_offset, _env_slice): (u32, &mut [u8]) =
            allocate_on_stack(&mut ctx, (MAX_ENV_VALUES * 4) as u32);
        let env_ptr = emscripten_memory_pointer!(memory.view(&ctx), env_offset) as *mut c_int;
        let pool_ptr = emscripten_memory_pointer!(memory.view(&ctx), pool_offset) as *mut u8;
        *env_ptr = pool_offset as i32;
        *environment = env_offset as i32;

        (pool_offset, env_ptr, pool_ptr)
    };

    // *env_ptr = 0;
    let default_vars = vec![
        ["USER", "web_user"],
        ["LOGNAME", "web_user"],
        ["PATH", "/"],
        ["PWD", "/"],
        ["HOME", "/home/web_user"],
        ["LANG", "C.UTF-8"],
        ["_", "thisProgram"],
    ];
    let mut strings = vec![];
    let mut total_size = 0;
    for [key, val] in &default_vars {
        let line = key.to_string() + "=" + val;
        total_size += line.len();
        strings.push(line);
    }
    if total_size as u32 > TOTAL_ENV_SIZE {
        panic!("Environment size exceeded TOTAL_ENV_SIZE!");
    }
    unsafe {
        for (i, s) in strings.iter().enumerate() {
            for (j, c) in s.chars().enumerate() {
                debug_assert!(c < u8::max_value() as char);
                *pool_ptr.add(j) = c as u8;
            }
            *env_ptr.add(i * 4) = pool_offset as i32;
            pool_offset += s.len() as u32 + 1;
            pool_ptr = pool_ptr.add(s.len() + 1);
        }
        *env_ptr.add(strings.len() * 4) = 0;
    }
}

pub fn ___assert_fail(_ctx: FunctionEnvMut<EmEnv>, _a: c_int, _b: c_int, _c: c_int, _d: c_int) {
    debug!("emscripten::___assert_fail {} {} {} {}", _a, _b, _c, _d);
    // TODO: Implement like emscripten expects regarding memory/page size
    // TODO raise an error
}

pub fn _pathconf(ctx: FunctionEnvMut<EmEnv>, path_addr: c_int, name: c_int) -> c_int {
    debug!(
        "emscripten::_pathconf {} {} - UNIMPLEMENTED",
        path_addr, name
    );
    let memory = ctx.data().memory(0);
    let _path = emscripten_memory_pointer!(memory.view(&ctx), path_addr) as *const c_char;
    match name {
        0 => 32000,
        1..=3 => 255,
        4 | 5 | 16 | 17 | 18 => 4096,
        6 | 7 | 20 => 1,
        8 => 0,
        9 | 10 | 11 | 12 | 14 | 15 | 19 => -1,
        13 => 64,
        _ => {
            // ___setErrNo(22);
            -1
        }
    }
}

pub fn _fpathconf(_ctx: FunctionEnvMut<EmEnv>, _fildes: c_int, name: c_int) -> c_int {
    debug!("emscripten::_fpathconf {} {}", _fildes, name);
    match name {
        0 => 32000,
        1..=3 => 255,
        4 | 5 | 16 | 17 | 18 => 4096,
        6 | 7 | 20 => 1,
        8 => 0,
        9 | 10 | 11 | 12 | 14 | 15 | 19 => -1,
        13 => 64,
        _ => {
            // ___setErrNo(22);
            -1
        }
    }
}

#[derive(Debug, Clone, Copy, ValueType)]
#[repr(C)]
pub struct EmAddrInfo {
    // int
    pub ai_flags: i32,
    // int
    pub ai_family: i32,
    // int
    pub ai_socktype: i32,
    // int
    pub ai_protocol: i32,
    // socklen_t
    pub ai_addrlen: u32,
    // struct sockaddr*
    pub ai_addr: WasmPtr<EmSockAddr>,
    // char*
    pub ai_canonname: WasmPtr<c_char>,
    // struct addrinfo*
    pub ai_next: WasmPtr<EmAddrInfo>,
}

// NOTE: from looking at emscripten JS, this should be a union
// TODO: review this, highly likely to have bugs
#[derive(Debug, Clone, Copy, ValueType)]
#[repr(C)]
pub struct EmSockAddr {
    pub sa_family: i16,
    pub sa_data: [c_char; 14],
}
