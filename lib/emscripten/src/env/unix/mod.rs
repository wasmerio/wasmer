/// NOTE: These syscalls only support wasm_32 for now because they take u32 offset
use libc::{c_int, getgrnam as libc_getgrnam, getpwnam as libc_getpwnam, sysconf};
use std::ffi::{CStr, CString};
use std::mem;
use std::os::raw::c_char;

use crate::env::{call_malloc, call_malloc_with_cast, EmAddrInfo, EmSockAddr};
use crate::utils::{copy_cstr_into_wasm, copy_terminated_array_of_cstrs};
use crate::EmEnv;
use wasmer::{FunctionEnvMut, WasmPtr};

// #[no_mangle]
/// emscripten: _getenv // (name: *const char) -> *const c_char;
pub fn _getenv(mut ctx: FunctionEnvMut<EmEnv>, name: i32) -> u32 {
    debug!("emscripten::_getenv");

    let em_env = ctx.data();
    let memory = em_env.memory(0);
    let name_addr = emscripten_memory_pointer!(memory.view(&ctx), name) as *const c_char;

    debug!("=> name({:?})", unsafe { CStr::from_ptr(name_addr) });

    let c_string = unsafe { CStr::from_ptr(name_addr) };
    let c_string = c_string.to_string_lossy();
    let env_var = em_env.get_env_var(c_string.as_ref());
    let env_var = match env_var {
        Some(s) => s,
        None => return 0,
    };
    let new_env_var = match CString::new(env_var) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    unsafe { copy_cstr_into_wasm(&mut ctx, new_env_var.as_ptr()) }
}

/// emscripten: _setenv // (name: *const char, name: *const value, overwrite: int);
pub fn _setenv(ctx: FunctionEnvMut<EmEnv>, name: c_int, value: c_int, overwrite: c_int) -> c_int {
    debug!("emscripten::_setenv");

    let em_env = ctx.data();
    let memory = em_env.memory(0);
    let name_addr = emscripten_memory_pointer!(memory.view(&ctx), name) as *const c_char;
    let value_addr = emscripten_memory_pointer!(memory.view(&ctx), value) as *const c_char;

    let name = unsafe { CStr::from_ptr(name_addr) }.to_string_lossy();
    let value = unsafe { CStr::from_ptr(value_addr) }.to_string_lossy();

    debug!("=> name({:?})", name);
    debug!("=> value({:?})", value);

    let previous_entry = em_env.set_env_var(name.as_ref(), value.as_ref());

    if let (0, Some(prev)) = (overwrite, previous_entry) {
        let _ = em_env.set_env_var(name.as_ref(), prev.as_ref());
    }

    0
}

/// emscripten: _putenv // (name: *const char);
pub fn _putenv(ctx: FunctionEnvMut<EmEnv>, name: c_int) -> c_int {
    debug!("emscripten::_putenv");

    let em_env = ctx.data();
    let memory = em_env.memory(0);
    let name_addr = emscripten_memory_pointer!(memory.view(&ctx), name) as *const c_char;

    let name = unsafe { CStr::from_ptr(name_addr) }.to_string_lossy();
    debug!("=> name({:?})", name);

    em_env.set_env_var(name.as_ref(), "");

    0
}

/// emscripten: _unsetenv // (name: *const char);
pub fn _unsetenv(mut ctx: FunctionEnvMut<EmEnv>, name: c_int) -> c_int {
    debug!("emscripten::_unsetenv");

    let name = {
        let em_env = ctx.data();
        let memory = em_env.memory(0);
        let name_addr = emscripten_memory_pointer!(memory.view(&ctx), name) as *const c_char;
        unsafe { CStr::from_ptr(name_addr) }
            .to_string_lossy()
            .to_string()
    };

    debug!("=> name({:?})", name);
    let em_env = ctx.data_mut();
    em_env.remove_env_var(name.as_ref());

    0
}

#[allow(clippy::cast_ptr_alignment)]
pub fn _getpwnam(mut ctx: FunctionEnvMut<EmEnv>, name_ptr: c_int) -> c_int {
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

    let memory = ctx.data().memory(0);
    let name = unsafe {
        let memory = memory.view(&ctx);
        let memory_name_ptr = emscripten_memory_pointer!(&memory, name_ptr) as *const c_char;
        CStr::from_ptr(memory_name_ptr)
    };

    unsafe {
        let passwd = &*libc_getpwnam(name.as_ptr());
        let passwd_struct_offset = call_malloc(&mut ctx, mem::size_of::<GuestPasswd>() as _);

        let memory = memory.view(&ctx);
        let passwd_struct_ptr =
            emscripten_memory_pointer!(&memory, passwd_struct_offset) as *mut GuestPasswd;
        (*passwd_struct_ptr).pw_name = copy_cstr_into_wasm(&mut ctx, passwd.pw_name);
        (*passwd_struct_ptr).pw_passwd = copy_cstr_into_wasm(&mut ctx, passwd.pw_passwd);
        (*passwd_struct_ptr).pw_gecos = copy_cstr_into_wasm(&mut ctx, passwd.pw_gecos);
        (*passwd_struct_ptr).pw_dir = copy_cstr_into_wasm(&mut ctx, passwd.pw_dir);
        (*passwd_struct_ptr).pw_shell = copy_cstr_into_wasm(&mut ctx, passwd.pw_shell);
        (*passwd_struct_ptr).pw_uid = passwd.pw_uid;
        (*passwd_struct_ptr).pw_gid = passwd.pw_gid;

        passwd_struct_offset as c_int
    }
}

#[allow(clippy::cast_ptr_alignment)]
pub fn _getgrnam(mut ctx: FunctionEnvMut<EmEnv>, name_ptr: c_int) -> c_int {
    debug!("emscripten::_getgrnam {}", name_ptr);

    #[repr(C)]
    struct GuestGroup {
        gr_name: u32,
        gr_passwd: u32,
        gr_gid: u32,
        gr_mem: u32,
    }

    let memory = ctx.data().memory(0);
    let name = unsafe {
        let memory_name_ptr =
            emscripten_memory_pointer!(memory.view(&ctx), name_ptr) as *const c_char;
        CStr::from_ptr(memory_name_ptr)
    };

    unsafe {
        let group = &*libc_getgrnam(name.as_ptr());
        let group_struct_offset = call_malloc(&mut ctx, mem::size_of::<GuestGroup>() as _);

        let group_struct_ptr =
            emscripten_memory_pointer!(memory.view(&ctx), group_struct_offset) as *mut GuestGroup;
        (*group_struct_ptr).gr_name = copy_cstr_into_wasm(&mut ctx, group.gr_name);
        (*group_struct_ptr).gr_passwd = copy_cstr_into_wasm(&mut ctx, group.gr_passwd);
        (*group_struct_ptr).gr_gid = group.gr_gid;
        (*group_struct_ptr).gr_mem = copy_terminated_array_of_cstrs(ctx, group.gr_mem);

        group_struct_offset as c_int
    }
}

pub fn _sysconf(_ctx: FunctionEnvMut<EmEnv>, name: c_int) -> i32 {
    debug!("emscripten::_sysconf {}", name);
    // TODO: Implement like emscripten expects regarding memory/page size
    unsafe { sysconf(name) as i32 } // TODO review i64
}

// this may be a memory leak, probably not though because emscripten does the same thing
pub fn _gai_strerror(mut ctx: FunctionEnvMut<EmEnv>, ecode: i32) -> i32 {
    debug!("emscripten::_gai_strerror({})", ecode);

    let cstr = unsafe { std::ffi::CStr::from_ptr(libc::gai_strerror(ecode)) };
    let bytes = cstr.to_bytes_with_nul();
    let string_on_guest: WasmPtr<c_char> = call_malloc_with_cast(&mut ctx, bytes.len() as _);
    let memory = ctx.data().memory(0);
    let memory = memory.view(&ctx);

    let writer = string_on_guest.slice(&memory, bytes.len() as _).unwrap();
    for (i, byte) in bytes.iter().enumerate() {
        writer.index(i as u64).write(*byte as _).unwrap();
    }

    string_on_guest.offset() as _
}

pub fn _getaddrinfo(
    mut ctx: FunctionEnvMut<EmEnv>,
    node_ptr: WasmPtr<c_char>,
    service_str_ptr: WasmPtr<c_char>,
    hints_ptr: WasmPtr<EmAddrInfo>,
    res_val_ptr: WasmPtr<WasmPtr<EmAddrInfo>>,
) -> i32 {
    use libc::{addrinfo, freeaddrinfo};
    debug!("emscripten::_getaddrinfo");
    debug!(" => node = {}", {
        if node_ptr.is_null() {
            std::borrow::Cow::Borrowed("null")
        } else {
            unimplemented!()
        }
    });
    debug!(" => server_str = {}", {
        if service_str_ptr.is_null() {
            std::borrow::Cow::Borrowed("null")
        } else {
            unimplemented!()
        }
    });

    let hints = if hints_ptr.is_null() {
        None
    } else {
        let memory = ctx.data().memory(0);
        let hints_guest = hints_ptr.deref(&memory.view(&ctx)).read().unwrap();
        Some(addrinfo {
            ai_flags: hints_guest.ai_flags,
            ai_family: hints_guest.ai_family,
            ai_socktype: hints_guest.ai_socktype,
            ai_protocol: hints_guest.ai_protocol,
            ai_addrlen: 0,
            ai_addr: std::ptr::null_mut(),
            ai_canonname: std::ptr::null_mut(),
            ai_next: std::ptr::null_mut(),
        })
    };

    let mut out_ptr: *mut addrinfo = std::ptr::null_mut();

    // allocate equivalent memory for res_val_ptr
    let result = unsafe {
        libc::getaddrinfo(
            if node_ptr.is_null() {
                std::ptr::null()
            } else {
                unimplemented!()
            },
            if service_str_ptr.is_null() {
                std::ptr::null()
            } else {
                unimplemented!()
            },
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
                call_malloc_with_cast(&mut ctx, std::mem::size_of::<EmAddrInfo>() as _);
            if head_of_list.is_none() {
                head_of_list = Some(current_guest_node_ptr);
            }

            // connect list
            if let Some(prev_guest) = previous_guest_node {
                let memory = ctx.data().memory(0);
                let memory = memory.view(&ctx);
                let derefed_prev_guest = prev_guest.deref(&memory);
                let mut pg = derefed_prev_guest.read().unwrap();
                pg.ai_next = current_guest_node_ptr;
                derefed_prev_guest.write(pg).unwrap();
            }

            // update values

            let host_addrlen = (*current_host_node).ai_addrlen;
            // allocate addr and copy data
            let guest_sockaddr_ptr = {
                let host_sockaddr_ptr = (*current_host_node).ai_addr;
                let guest_sockaddr_ptr: WasmPtr<EmSockAddr> =
                    call_malloc_with_cast(&mut ctx, host_addrlen as _);

                let memory = ctx.data().memory(0);
                let memory = memory.view(&ctx);
                let derefed_guest_sockaddr = guest_sockaddr_ptr.deref(&memory);
                let mut gs = derefed_guest_sockaddr.read().unwrap();
                gs.sa_family = (*host_sockaddr_ptr).sa_family as i16;
                gs.sa_data = (*host_sockaddr_ptr).sa_data;
                derefed_guest_sockaddr.write(gs).unwrap();

                guest_sockaddr_ptr
            };

            // allocate canon name on guest and copy data over
            let guest_canonname_ptr = {
                let str_ptr = (*current_host_node).ai_canonname;
                if !str_ptr.is_null() {
                    let canonname_cstr = std::ffi::CStr::from_ptr(str_ptr);
                    let canonname_bytes = canonname_cstr.to_bytes_with_nul();
                    let str_size = canonname_bytes.len();
                    let guest_canonname: WasmPtr<c_char> =
                        call_malloc_with_cast(&mut ctx, str_size as _);

                    let memory = ctx.data().memory(0);
                    let memory = memory.view(&ctx);
                    let guest_canonname_writer =
                        guest_canonname.slice(&memory, str_size as _).unwrap();
                    for (i, b) in canonname_bytes.iter().enumerate() {
                        guest_canonname_writer
                            .index(i as u64)
                            .write(*b as _)
                            .unwrap();
                    }

                    guest_canonname
                } else {
                    WasmPtr::new(0)
                }
            };

            let memory = ctx.data().memory(0);
            let memory = memory.view(&ctx);
            let derefed_current_guest_node = current_guest_node_ptr.deref(&memory);
            let mut cgn = derefed_current_guest_node.read().unwrap();
            cgn.ai_flags = (*current_host_node).ai_flags;
            cgn.ai_family = (*current_host_node).ai_family;
            cgn.ai_socktype = (*current_host_node).ai_socktype;
            cgn.ai_protocol = (*current_host_node).ai_protocol;
            cgn.ai_addrlen = host_addrlen;
            cgn.ai_addr = guest_sockaddr_ptr;
            cgn.ai_canonname = guest_canonname_ptr;
            cgn.ai_next = WasmPtr::new(0);
            derefed_current_guest_node.write(cgn).unwrap();

            previous_guest_node = Some(current_guest_node_ptr);
            current_host_node = (*current_host_node).ai_next;
        }
        // this frees all connected nodes on the linked list
        freeaddrinfo(out_ptr);
        head_of_list.unwrap_or_else(|| WasmPtr::new(0))
    };

    let memory = ctx.data().memory(0);
    res_val_ptr
        .deref(&memory.view(&ctx))
        .write(head_of_list)
        .unwrap();

    0
}
