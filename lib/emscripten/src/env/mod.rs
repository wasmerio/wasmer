#[cfg(unix)]
mod unix;

#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use self::unix::*;

#[cfg(windows)]
pub use self::windows::*;

use libc::c_char;

use crate::{allocate_on_stack, EmscriptenData};

use std::cell::Cell;
use std::os::raw::c_int;
use wasmer_runtime_core::{
    memory::ptr::{Array, WasmPtr},
    types::ValueType,
    vm::Ctx,
};

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct EmAddrInfo {
    // int
    ai_flags: i32,
    // int
    ai_family: i32,
    // int
    ai_socktype: i32,
    // int
    ai_protocol: i32,
    // socklen_t
    ai_addrlen: u32,
    // struct sockaddr*
    ai_addr: WasmPtr<EmSockAddr>,
    // char*
    ai_canonname: WasmPtr<c_char, Array>,
    // struct addrinfo*
    ai_next: WasmPtr<EmAddrInfo>,
}

unsafe impl ValueType for EmAddrInfo {}

// NOTE: from looking at emscripten JS, this should be a union
// TODO: review this, highly likely to have bugs
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct EmSockAddr {
    sa_family: i16,
    sa_data: [c_char; 14],
}

unsafe impl ValueType for EmSockAddr {}

pub fn _getaddrinfo(
    ctx: &mut Ctx,
    node_ptr: WasmPtr<c_char>,
    service_str_ptr: WasmPtr<c_char>,
    hints_ptr: WasmPtr<EmAddrInfo>,
    res_val_ptr: WasmPtr<WasmPtr<EmAddrInfo>>,
) -> i32 {
    use libc::{addrinfo, freeaddrinfo};
    debug!("emscripten::_getaddrinfo");
    let memory = ctx.memory(0);
    debug!(" => node = {}", unsafe {
        node_ptr
            .deref(memory)
            .map(|np| {
                std::ffi::CStr::from_ptr(np as *const Cell<c_char> as *const c_char)
                    .to_string_lossy()
            })
            .unwrap_or(std::borrow::Cow::Borrowed("null"))
    });
    debug!(" => server_str = {}", unsafe {
        service_str_ptr
            .deref(memory)
            .map(|np| {
                std::ffi::CStr::from_ptr(np as *const Cell<c_char> as *const c_char)
                    .to_string_lossy()
            })
            .unwrap_or(std::borrow::Cow::Borrowed("null"))
    });

    let hints = hints_ptr.deref(memory).map(|hints_memory| {
        let hints_guest = dbg!(hints_memory.get());
        unsafe {
            let mut hints_native: addrinfo = std::mem::uninitialized();
            hints_native.ai_flags = hints_guest.ai_flags;
            hints_native.ai_family = hints_guest.ai_family;
            hints_native.ai_socktype = hints_guest.ai_socktype;
            hints_native.ai_protocol = hints_guest.ai_protocol;
            hints_native.ai_addrlen = 0;
            hints_native.ai_addr = std::ptr::null_mut();
            hints_native.ai_canonname = std::ptr::null_mut();
            hints_native.ai_next = std::ptr::null_mut();

            hints_native
        }
    });

    let mut out_ptr: *mut addrinfo = std::ptr::null_mut();

    // allocate equivalent memory for res_val_ptr
    let result = unsafe {
        libc::getaddrinfo(
            (node_ptr
                .deref(memory)
                .map(|m| m as *const Cell<c_char> as *const c_char))
            .unwrap_or(std::ptr::null()),
            (service_str_ptr
                .deref(memory)
                .map(|m| m as *const Cell<c_char> as *const c_char))
            .unwrap_or(std::ptr::null()),
            hints
                .as_ref()
                .map(|h| h as *const addrinfo)
                .unwrap_or(std::ptr::null()),
            &mut out_ptr as *mut *mut addrinfo,
        )
    };
    if dbg!(result) != 0 {
        return result;
    }

    // walk linked list and copy over, freeing them from the kernel
    let head_of_list = unsafe {
        let mut current_host_node = out_ptr;
        let mut head_of_list = None;
        let mut previous_guest_node: Option<WasmPtr<EmAddrInfo>> = None;

        while !current_host_node.is_null() {
            let current_guest_node_ptr: WasmPtr<EmAddrInfo> =
                call_malloc_with_cast(ctx, std::mem::size_of::<EmAddrInfo>() as _);
            if head_of_list.is_none() {
                dbg!("Setting head of list");
                head_of_list = Some(current_guest_node_ptr);
            }

            // connect list
            if let Some(prev_guest) = previous_guest_node {
                let mut pg = prev_guest.deref_mut(ctx.memory(0)).unwrap().get_mut();
                pg.ai_next = current_guest_node_ptr;
                dbg!("list connected");
            }

            // update values

            let host_addrlen = (*current_host_node).ai_addrlen;
            // allocate addr and copy data
            let guest_sockaddr_ptr = {
                let host_sockaddr_ptr = (*current_host_node).ai_addr;
                let guest_sockaddr_ptr: WasmPtr<EmSockAddr> =
                    call_malloc_with_cast(ctx, host_addrlen as _);
                let guest_sockaddr = guest_sockaddr_ptr
                    .deref_mut(ctx.memory(0))
                    .unwrap()
                    .get_mut();

                guest_sockaddr.sa_family = (*host_sockaddr_ptr).sa_family as i16;
                guest_sockaddr.sa_data = (*host_sockaddr_ptr).sa_data.clone();
                guest_sockaddr_ptr
            };

            dbg!("Socketaddr allocated");

            // allocate canon name on guest and copy data over
            let guest_canonname_ptr = {
                let str_ptr = (*current_host_node).ai_canonname;
                if !str_ptr.is_null() {
                    let canonname_cstr = dbg!(std::ffi::CStr::from_ptr(str_ptr));
                    let canonname_bytes = canonname_cstr.to_bytes_with_nul();
                    let str_size = dbg!(canonname_bytes.len());
                    let guest_canonname: WasmPtr<c_char, Array> =
                        call_malloc_with_cast(ctx, str_size as _);

                    let guest_canonname_writer = guest_canonname
                        .deref(ctx.memory(0), 0, str_size as _)
                        .unwrap();
                    for (i, b) in canonname_bytes.into_iter().enumerate() {
                        guest_canonname_writer[i].set(*b as i8)
                    }

                    guest_canonname
                } else {
                    WasmPtr::new(0)
                }
            };

            dbg!("canonname allocated");

            let mut current_guest_node = current_guest_node_ptr
                .deref_mut(ctx.memory(0))
                .unwrap()
                .get_mut();
            // TODO order these
            current_guest_node.ai_flags = (*current_host_node).ai_flags;
            current_guest_node.ai_family = (*current_host_node).ai_family;
            current_guest_node.ai_socktype = (*current_host_node).ai_socktype;
            current_guest_node.ai_protocol = (*current_host_node).ai_protocol;
            current_guest_node.ai_addrlen = host_addrlen;
            current_guest_node.ai_addr = guest_sockaddr_ptr;
            current_guest_node.ai_canonname = guest_canonname_ptr;
            current_guest_node.ai_next = WasmPtr::new(0);

            dbg!("Guest node updated");

            previous_guest_node = Some(current_guest_node_ptr);
            current_host_node = (*current_host_node).ai_next;
            dbg!("End of loop bookkeeping finished");
        }

        dbg!("freeing memory");
        // this frees all connected nodes on the linked list
        freeaddrinfo(out_ptr);
        head_of_list.unwrap_or(WasmPtr::new(0))
    };

    res_val_ptr
        .deref(ctx.memory(0))
        .unwrap()
        .set(dbg!(head_of_list));

    0
}

pub fn call_malloc(ctx: &mut Ctx, size: u32) -> u32 {
    get_emscripten_data(ctx).malloc.call(size).unwrap()
}

pub fn call_malloc_with_cast<T: Copy, Ty>(ctx: &mut Ctx, size: u32) -> WasmPtr<T, Ty> {
    WasmPtr::new(get_emscripten_data(ctx).malloc.call(size).unwrap())
}

pub fn call_memalign(ctx: &mut Ctx, alignment: u32, size: u32) -> u32 {
    if let Some(memalign) = &get_emscripten_data(ctx).memalign {
        memalign.call(alignment, size).unwrap()
    } else {
        panic!("Memalign is set to None");
    }
}

pub fn call_memset(ctx: &mut Ctx, pointer: u32, value: u32, size: u32) -> u32 {
    get_emscripten_data(ctx)
        .memset
        .call(pointer, value, size)
        .unwrap()
}

pub(crate) fn get_emscripten_data(ctx: &mut Ctx) -> &mut EmscriptenData {
    unsafe { &mut *(ctx.data as *mut EmscriptenData) }
}

pub fn _getpagesize(_ctx: &mut Ctx) -> u32 {
    debug!("emscripten::_getpagesize");
    16384
}

pub fn _times(ctx: &mut Ctx, buffer: u32) -> u32 {
    if buffer != 0 {
        call_memset(ctx, buffer, 0, 16);
    }
    0
}

#[allow(clippy::cast_ptr_alignment)]
pub fn ___build_environment(ctx: &mut Ctx, environ: c_int) {
    debug!("emscripten::___build_environment {}", environ);
    const MAX_ENV_VALUES: u32 = 64;
    const TOTAL_ENV_SIZE: u32 = 1024;
    let environment = emscripten_memory_pointer!(ctx.memory(0), environ) as *mut c_int;
    let (mut pool_offset, env_ptr, mut pool_ptr) = unsafe {
        let (pool_offset, _pool_slice): (u32, &mut [u8]) =
            allocate_on_stack(ctx, TOTAL_ENV_SIZE as u32);
        let (env_offset, _env_slice): (u32, &mut [u8]) =
            allocate_on_stack(ctx, (MAX_ENV_VALUES * 4) as u32);
        let env_ptr = emscripten_memory_pointer!(ctx.memory(0), env_offset) as *mut c_int;
        let pool_ptr = emscripten_memory_pointer!(ctx.memory(0), pool_offset) as *mut u8;
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

pub fn ___assert_fail(_ctx: &mut Ctx, _a: c_int, _b: c_int, _c: c_int, _d: c_int) {
    debug!("emscripten::___assert_fail {} {} {} {}", _a, _b, _c, _d);
    // TODO: Implement like emscripten expects regarding memory/page size
    // TODO raise an error
}

pub fn _pathconf(ctx: &mut Ctx, path_addr: c_int, name: c_int) -> c_int {
    debug!(
        "emscripten::_pathconf {} {} - UNIMPLEMENTED",
        path_addr, name
    );
    let _path = emscripten_memory_pointer!(ctx.memory(0), path_addr) as *const c_char;
    match name {
        0 => 32000,
        1 | 2 | 3 => 255,
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

pub fn _fpathconf(_ctx: &mut Ctx, _fildes: c_int, name: c_int) -> c_int {
    debug!("emscripten::_fpathconf {} {}", _fildes, name);
    match name {
        0 => 32000,
        1 | 2 | 3 => 255,
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
