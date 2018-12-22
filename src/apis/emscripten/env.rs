use super::super::host;
/// NOTE: These syscalls only support wasm_32 for now because they take u32 offset
use libc::{
    c_int, c_long, getenv, getgrnam as libc_getgrnam, getpwnam as libc_getpwnam, putenv, setenv,
    sysconf, unsetenv,
};
use std::ffi::CStr;
use std::mem;
use std::os::raw::c_char;

use super::utils::{allocate_on_stack, copy_cstr_into_wasm, copy_terminated_array_of_cstrs};
use crate::webassembly::Instance;

// #[no_mangle]
/// emscripten: _getenv // (name: *const char) -> *const c_char;
pub extern "C" fn _getenv(name: c_int, instance: &mut Instance) -> u32 {
    debug!("emscripten::_getenv");

    let name_addr = instance.memory_offset_addr(0, name as usize) as *const c_char;

    debug!("=> name({:?})", unsafe { CStr::from_ptr(name_addr) });

    let c_str = unsafe { getenv(name_addr) };
    if c_str.is_null() {
        return 0;
    }

    unsafe { copy_cstr_into_wasm(instance, c_str) }
}

/// emscripten: _setenv // (name: *const char, name: *const value, overwrite: int);
pub extern "C" fn _setenv(name: c_int, value: c_int, overwrite: c_int, instance: &mut Instance) {
    debug!("emscripten::_setenv");

    let name_addr = instance.memory_offset_addr(0, name as usize) as *const c_char;
    let value_addr = instance.memory_offset_addr(0, value as usize) as *const c_char;

    debug!("=> name({:?})", unsafe { CStr::from_ptr(name_addr) });
    debug!("=> value({:?})", unsafe { CStr::from_ptr(value_addr) });

    unsafe { setenv(name_addr, value_addr, overwrite) };
}

/// emscripten: _putenv // (name: *const char);
pub extern "C" fn _putenv(name: c_int, instance: &mut Instance) {
    debug!("emscripten::_putenv");

    let name_addr = instance.memory_offset_addr(0, name as usize) as *const c_char;

    debug!("=> name({:?})", unsafe { CStr::from_ptr(name_addr) });

    unsafe { putenv(name_addr as _) };
}

/// emscripten: _unsetenv // (name: *const char);
pub extern "C" fn _unsetenv(name: c_int, instance: &mut Instance) {
    debug!("emscripten::_unsetenv");

    let name_addr = instance.memory_offset_addr(0, name as usize) as *const c_char;

    debug!("=> name({:?})", unsafe { CStr::from_ptr(name_addr) });

    unsafe { unsetenv(name_addr) };
}

pub extern "C" fn _getpwnam(name_ptr: c_int, instance: &mut Instance) -> c_int {
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
        let memory_name_ptr = instance.memory_offset_addr(0, name_ptr as usize) as *const c_char;
        CStr::from_ptr(memory_name_ptr)
    };

    unsafe {
        let passwd = &*libc_getpwnam(name.as_ptr());
        let passwd_struct_offset = (instance.emscripten_data.as_ref().unwrap().malloc)(
            mem::size_of::<GuestPasswd>() as _,
            instance,
        );

        let passwd_struct_ptr =
            instance.memory_offset_addr(0, passwd_struct_offset as _) as *mut GuestPasswd;
        (*passwd_struct_ptr).pw_name = copy_cstr_into_wasm(instance, passwd.pw_name);
        (*passwd_struct_ptr).pw_passwd = copy_cstr_into_wasm(instance, passwd.pw_passwd);
        (*passwd_struct_ptr).pw_gecos = copy_cstr_into_wasm(instance, passwd.pw_gecos);
        (*passwd_struct_ptr).pw_dir = copy_cstr_into_wasm(instance, passwd.pw_dir);
        (*passwd_struct_ptr).pw_shell = copy_cstr_into_wasm(instance, passwd.pw_shell);
        (*passwd_struct_ptr).pw_uid = passwd.pw_uid;
        (*passwd_struct_ptr).pw_gid = passwd.pw_gid;

        passwd_struct_offset as c_int
    }
}

pub extern "C" fn _getgrnam(name_ptr: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::_getgrnam {}", name_ptr);

    #[repr(C)]
    struct GuestGroup {
        gr_name: u32,
        gr_passwd: u32,
        gr_gid: u32,
        gr_mem: u32,
    }

    let name = unsafe {
        let memory_name_ptr = instance.memory_offset_addr(0, name_ptr as usize) as *const c_char;
        CStr::from_ptr(memory_name_ptr)
    };

    unsafe {
        let group = &*libc_getgrnam(name.as_ptr());
        let group_struct_offset = (instance.emscripten_data.as_ref().unwrap().malloc)(
            mem::size_of::<GuestGroup>() as _,
            instance,
        );

        let group_struct_ptr =
            instance.memory_offset_addr(0, group_struct_offset as _) as *mut GuestGroup;
        (*group_struct_ptr).gr_name = copy_cstr_into_wasm(instance, group.gr_name);
        (*group_struct_ptr).gr_passwd = copy_cstr_into_wasm(instance, group.gr_passwd);
        (*group_struct_ptr).gr_gid = group.gr_gid;
        (*group_struct_ptr).gr_mem = copy_terminated_array_of_cstrs(instance, group.gr_mem);

        group_struct_offset as c_int
    }
}

pub extern "C" fn _getpagesize() -> u32 {
    debug!("emscripten::_getpagesize");
    16384
}

pub extern "C" fn ___build_environment(environ: c_int, instance: &mut Instance) {
    debug!("emscripten::___build_environment {}", environ);
    const MAX_ENV_VALUES: u32 = 64;
    const TOTAL_ENV_SIZE: u32 = 1024;
    let mut environment = instance.memory_offset_addr(0, environ as _) as *mut c_int;
    unsafe {
        let (pool_offset, pool_slice): (u32, &mut [u8]) =
            allocate_on_stack(TOTAL_ENV_SIZE as u32, instance);
        let (env_offset, env_slice): (u32, &mut [u8]) =
            allocate_on_stack((MAX_ENV_VALUES * 4) as u32, instance);
        let mut env_ptr = instance.memory_offset_addr(0, env_offset as _) as *mut c_int;
        let mut pool_ptr = instance.memory_offset_addr(0, pool_offset as _) as *mut c_int;
        *env_ptr = pool_offset as i32;
        *environment = env_offset as i32;

        // *env_ptr = 0;
    };
    // unsafe {
    //     *env_ptr = 0;
    // };
}

pub extern "C" fn _sysconf(name: c_int, _instance: &mut Instance) -> c_long {
    debug!("emscripten::_sysconf {}", name);
    // TODO: Implement like emscripten expects regarding memory/page size
    unsafe { sysconf(name) }
}
