/// NOTE: These syscalls only support wasm_32 for now because they take u32 offset
use libc::{
    c_int, getenv, getgrnam as libc_getgrnam, getpwnam as libc_getpwnam, putenv, setenv, sysconf,
    unsetenv,
};
use std::ffi::CStr;
use std::mem;
use std::os::raw::c_char;

use crate::env::call_malloc;
use crate::utils::{copy_cstr_into_wasm, copy_terminated_array_of_cstrs};
use wasmer_runtime_core::vm::Ctx;

// #[no_mangle]
/// emscripten: _getenv // (name: *const char) -> *const c_char;
pub fn _getenv(ctx: &mut Ctx, name: i32) -> u32 {
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
pub fn _setenv(ctx: &mut Ctx, name: c_int, value: c_int, overwrite: c_int) -> c_int {
    debug!("emscripten::_setenv");

    let name_addr = emscripten_memory_pointer!(ctx.memory(0), name) as *const c_char;
    let value_addr = emscripten_memory_pointer!(ctx.memory(0), value) as *const c_char;

    debug!("=> name({:?})", unsafe { CStr::from_ptr(name_addr) });
    debug!("=> value({:?})", unsafe { CStr::from_ptr(value_addr) });

    unsafe { setenv(name_addr, value_addr, overwrite) }
}

/// emscripten: _putenv // (name: *const char);
pub fn _putenv(ctx: &mut Ctx, name: c_int) -> c_int {
    debug!("emscripten::_putenv");

    let name_addr = emscripten_memory_pointer!(ctx.memory(0), name) as *const c_char;

    debug!("=> name({:?})", unsafe { CStr::from_ptr(name_addr) });

    unsafe { putenv(name_addr as _) }
}

/// emscripten: _unsetenv // (name: *const char);
pub fn _unsetenv(ctx: &mut Ctx, name: c_int) -> c_int {
    debug!("emscripten::_unsetenv");

    let name_addr = emscripten_memory_pointer!(ctx.memory(0), name) as *const c_char;

    debug!("=> name({:?})", unsafe { CStr::from_ptr(name_addr) });

    unsafe { unsetenv(name_addr) }
}

#[allow(clippy::cast_ptr_alignment)]
pub fn _getpwnam(ctx: &mut Ctx, name_ptr: c_int) -> c_int {
    debug!("emscripten::_getpwnam {}", name_ptr);

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
pub fn _getgrnam(ctx: &mut Ctx, name_ptr: c_int) -> c_int {
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

pub fn _sysconf(_ctx: &mut Ctx, name: c_int) -> i32 {
    debug!("emscripten::_sysconf {}", name);
    // TODO: Implement like emscripten expects regarding memory/page size
    unsafe { sysconf(name) as i32 } // TODO review i64
}
