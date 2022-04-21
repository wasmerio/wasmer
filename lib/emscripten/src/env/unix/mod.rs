/// NOTE: These syscalls only support wasm_32 for now because they take u32 offset
use libc::{
    c_int, getenv, getgrnam as libc_getgrnam, getpwnam as libc_getpwnam, putenv, setenv, sysconf,
    unsetenv,
};
use std::ffi::CStr;
use std::mem;
use std::os::raw::c_char;

use crate::env::{call_malloc, call_malloc_with_cast, EmAddrInfo, EmSockAddr};
use crate::ptr::{Array, WasmPtr};
use crate::utils::{copy_cstr_into_wasm, copy_terminated_array_of_cstrs};
use crate::EmEnv;

// #[no_mangle]
/// emscripten: _getenv // (name: *const char) -> *const c_char;
pub fn _getenv(ctx: &EmEnv, name: i32) -> u32 {
    debug!("emscripten::_getenv");

    let name_addr = emscripten_memory_pointer!(ctx.memory(0), name) as *const c_char;

    debug!("=> name({:?})", unsafe { CStr::from_ptr(name_addr) });

    let c_str = unsafe { getenv(name_addr) };
    if c_str.is_null() {
        return 0;
    }

    unsafe { copy_cstr_into_wasm(ctx, c_str) }
}

/// emscripten: _setenv // (name: *const char, name: *const value, overwrite: int);
pub fn _setenv(ctx: &EmEnv, name: c_int, value: c_int, overwrite: c_int) -> c_int {
    debug!("emscripten::_setenv");

    let name_addr = emscripten_memory_pointer!(ctx.memory(0), name) as *const c_char;
    let value_addr = emscripten_memory_pointer!(ctx.memory(0), value) as *const c_char;

    debug!("=> name({:?})", unsafe { CStr::from_ptr(name_addr) });
    debug!("=> value({:?})", unsafe { CStr::from_ptr(value_addr) });

    unsafe { setenv(name_addr, value_addr, overwrite) }
}

/// emscripten: _putenv // (name: *const char);
pub fn _putenv(ctx: &EmEnv, name: c_int) -> c_int {
    debug!("emscripten::_putenv");

    let name_addr = emscripten_memory_pointer!(ctx.memory(0), name) as *const c_char;

    debug!("=> name({:?})", unsafe { CStr::from_ptr(name_addr) });

    unsafe { putenv(name_addr as _) }
}

/// emscripten: _unsetenv // (name: *const char);
pub fn _unsetenv(ctx: &EmEnv, name: c_int) -> c_int {
    debug!("emscripten::_unsetenv");

    let name_addr = emscripten_memory_pointer!(ctx.memory(0), name) as *const c_char;

    debug!("=> name({:?})", unsafe { CStr::from_ptr(name_addr) });

    unsafe { unsetenv(name_addr) }
}

#[allow(clippy::cast_ptr_alignment)]
pub fn _getpwnam(ctx: &EmEnv, name_ptr: c_int) -> c_int {
    debug!("emscripten::_getpwnam {}", name_ptr);
    #[cfg(feature = "debug")]
    let _ = name_ptr;

    #[repr(C)]
    struct GuestPasswd {
        pw_name: u32,
        pw_passwd: u32,
        pw_uid: u32,
        pw_gid: u32,
        pw_gecos: u32,
        pw_dir: u32,
        pw_shell: u32,
    }

    let name = unsafe {
        let memory_name_ptr = emscripten_memory_pointer!(ctx.memory(0), name_ptr) as *const c_char;
        CStr::from_ptr(memory_name_ptr)
    };

    unsafe {
        let passwd = &*libc_getpwnam(name.as_ptr());
        let passwd_struct_offset = call_malloc(ctx, mem::size_of::<GuestPasswd>() as _);

        let passwd_struct_ptr =
            emscripten_memory_pointer!(ctx.memory(0), passwd_struct_offset) as *mut GuestPasswd;
        (*passwd_struct_ptr).pw_name = copy_cstr_into_wasm(ctx, passwd.pw_name);
        (*passwd_struct_ptr).pw_passwd = copy_cstr_into_wasm(ctx, passwd.pw_passwd);
        (*passwd_struct_ptr).pw_gecos = copy_cstr_into_wasm(ctx, passwd.pw_gecos);
        (*passwd_struct_ptr).pw_dir = copy_cstr_into_wasm(ctx, passwd.pw_dir);
        (*passwd_struct_ptr).pw_shell = copy_cstr_into_wasm(ctx, passwd.pw_shell);
        (*passwd_struct_ptr).pw_uid = passwd.pw_uid;
        (*passwd_struct_ptr).pw_gid = passwd.pw_gid;

        passwd_struct_offset as c_int
    }
}

#[allow(clippy::cast_ptr_alignment)]
pub fn _getgrnam(ctx: &EmEnv, name_ptr: c_int) -> c_int {
    debug!("emscripten::_getgrnam {}", name_ptr);

    #[repr(C)]
    struct GuestGroup {
        gr_name: u32,
        gr_passwd: u32,
        gr_gid: u32,
        gr_mem: u32,
    }

    let name = unsafe {
        let memory_name_ptr = emscripten_memory_pointer!(ctx.memory(0), name_ptr) as *const c_char;
        CStr::from_ptr(memory_name_ptr)
    };

    unsafe {
        let group = &*libc_getgrnam(name.as_ptr());
        let group_struct_offset = call_malloc(ctx, mem::size_of::<GuestGroup>() as _);

        let group_struct_ptr =
            emscripten_memory_pointer!(ctx.memory(0), group_struct_offset) as *mut GuestGroup;
        (*group_struct_ptr).gr_name = copy_cstr_into_wasm(ctx, group.gr_name);
        (*group_struct_ptr).gr_passwd = copy_cstr_into_wasm(ctx, group.gr_passwd);
        (*group_struct_ptr).gr_gid = group.gr_gid;
        (*group_struct_ptr).gr_mem = copy_terminated_array_of_cstrs(ctx, group.gr_mem);

        group_struct_offset as c_int
    }
}

pub fn _sysconf(_ctx: &EmEnv, name: c_int) -> i32 {
    debug!("emscripten::_sysconf {}", name);
    // TODO: Implement like emscripten expects regarding memory/page size
    unsafe { sysconf(name) as i32 } // TODO review i64
}

// this may be a memory leak, probably not though because emscripten does the same thing
pub fn _gai_strerror(ctx: &EmEnv, ecode: i32) -> i32 {
    debug!("emscripten::_gai_strerror({})", ecode);

    let cstr = unsafe { std::ffi::CStr::from_ptr(libc::gai_strerror(ecode)) };
    let bytes = cstr.to_bytes_with_nul();
    let string_on_guest: WasmPtr<c_char, Array> = call_malloc_with_cast(ctx, bytes.len() as _);
    let memory = ctx.memory(0);

    let writer = string_on_guest.deref(&memory, 0, bytes.len() as _).unwrap();
    for (i, byte) in bytes.iter().enumerate() {
        writer[i].set(*byte as _);
    }

    string_on_guest.offset() as _
}

pub fn _getaddrinfo(
    ctx: &EmEnv,
    node_ptr: WasmPtr<c_char>,
    service_str_ptr: WasmPtr<c_char>,
    hints_ptr: WasmPtr<EmAddrInfo>,
    res_val_ptr: WasmPtr<WasmPtr<EmAddrInfo>>,
) -> i32 {
    use libc::{addrinfo, freeaddrinfo};
    debug!("emscripten::_getaddrinfo");
    let memory = ctx.memory(0);
    debug!(" => node = {}", {
        node_ptr
            .deref(&memory)
            .map(|_np| {
                unimplemented!();
                // std::ffi::CStr::from_ptr(np as *const Cell<c_char> as *const c_char)
                //     .to_string_lossy()
            })
            .unwrap_or(std::borrow::Cow::Borrowed("null"))
    });
    debug!(" => server_str = {}", {
        service_str_ptr
            .deref(&memory)
            .map(|_np| {
                unimplemented!();
                // std::ffi::CStr::from_ptr(np as *const Cell<c_char> as *const c_char)
                //     .to_string_lossy()
            })
            .unwrap_or(std::borrow::Cow::Borrowed("null"))
    });

    let hints = hints_ptr.deref(&memory).map(|hints_memory| {
        let hints_guest = hints_memory.get();
        addrinfo {
            ai_flags: hints_guest.ai_flags,
            ai_family: hints_guest.ai_family,
            ai_socktype: hints_guest.ai_socktype,
            ai_protocol: hints_guest.ai_protocol,
            ai_addrlen: 0,
            ai_addr: std::ptr::null_mut(),
            ai_canonname: std::ptr::null_mut(),
            ai_next: std::ptr::null_mut(),
        }
    });

    let mut out_ptr: *mut addrinfo = std::ptr::null_mut();

    // allocate equivalent memory for res_val_ptr
    let result = unsafe {
        libc::getaddrinfo(
            (node_ptr.deref(&memory).map(|_m| {
                unimplemented!();
                //m as *const Cell<c_char> as *const c_char
            }))
            .unwrap_or(std::ptr::null()),
            service_str_ptr
                .deref(&memory)
                .map(|_m| {
                    unimplemented!();
                    // m as *const Cell<c_char> as *const c_char
                })
                .unwrap_or(std::ptr::null()),
            hints
                .as_ref()
                .map(|h| h as *const addrinfo)
                .unwrap_or(std::ptr::null()),
            &mut out_ptr as *mut *mut addrinfo,
        )
    };
    if result != 0 {
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
                head_of_list = Some(current_guest_node_ptr);
            }

            // connect list
            if let Some(prev_guest) = previous_guest_node {
                let derefed_prev_guest = prev_guest.deref(&memory).unwrap();
                let mut pg = derefed_prev_guest.get();
                pg.ai_next = current_guest_node_ptr;
                derefed_prev_guest.set(pg);
            }

            // update values

            let host_addrlen = (*current_host_node).ai_addrlen;
            // allocate addr and copy data
            let guest_sockaddr_ptr = {
                let host_sockaddr_ptr = (*current_host_node).ai_addr;
                let guest_sockaddr_ptr: WasmPtr<EmSockAddr> =
                    call_malloc_with_cast(ctx, host_addrlen as _);

                let derefed_guest_sockaddr = guest_sockaddr_ptr.deref(&memory).unwrap();
                let mut gs = derefed_guest_sockaddr.get();
                gs.sa_family = (*host_sockaddr_ptr).sa_family as i16;
                gs.sa_data = (*host_sockaddr_ptr).sa_data;
                derefed_guest_sockaddr.set(gs);

                guest_sockaddr_ptr
            };

            // allocate canon name on guest and copy data over
            let guest_canonname_ptr = {
                let str_ptr = (*current_host_node).ai_canonname;
                if !str_ptr.is_null() {
                    let canonname_cstr = std::ffi::CStr::from_ptr(str_ptr);
                    let canonname_bytes = canonname_cstr.to_bytes_with_nul();
                    let str_size = canonname_bytes.len();
                    let guest_canonname: WasmPtr<c_char, Array> =
                        call_malloc_with_cast(ctx, str_size as _);

                    let guest_canonname_writer =
                        guest_canonname.deref(&memory, 0, str_size as _).unwrap();
                    for (i, b) in canonname_bytes.iter().enumerate() {
                        guest_canonname_writer[i].set(*b as _)
                    }

                    guest_canonname
                } else {
                    WasmPtr::new(0)
                }
            };

            let derefed_current_guest_node = current_guest_node_ptr.deref(&memory).unwrap();
            let mut cgn = derefed_current_guest_node.get();
            cgn.ai_flags = (*current_host_node).ai_flags;
            cgn.ai_family = (*current_host_node).ai_family;
            cgn.ai_socktype = (*current_host_node).ai_socktype;
            cgn.ai_protocol = (*current_host_node).ai_protocol;
            cgn.ai_addrlen = host_addrlen;
            cgn.ai_addr = guest_sockaddr_ptr;
            cgn.ai_canonname = guest_canonname_ptr;
            cgn.ai_next = WasmPtr::new(0);
            derefed_current_guest_node.set(cgn);

            previous_guest_node = Some(current_guest_node_ptr);
            current_host_node = (*current_host_node).ai_next;
        }
        // this frees all connected nodes on the linked list
        freeaddrinfo(out_ptr);
        head_of_list.unwrap_or_else(|| WasmPtr::new(0))
    };

    res_val_ptr.deref(&memory).unwrap().set(head_of_list);

    0
}
