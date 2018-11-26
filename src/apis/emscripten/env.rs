use super::super::host;
/// NOTE: These syscalls only support wasm_32 for now because they take u32 offset
use libc::{c_int, getpwnam as libc_getpwnam, passwd, getgrnam as libc_getgrnam, group};
use std::ffi::CStr;
use std::os::raw::c_char;
use std::{slice, mem};

use crate::webassembly::Instance;
use super::utils::{copy_cstr_into_wasm, copy_terminated_array_of_cstrs};

/// emscripten: _getenv
pub extern "C" fn _getenv(name_ptr: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::_getenv {}", name_ptr);
    let name = unsafe {
        let memory_name_ptr = instance.memory_offset_addr(0, name_ptr as usize) as *const c_char;
        CStr::from_ptr(memory_name_ptr).to_str().unwrap()
    };
    match host::get_env(name, instance) {
        Ok(_) => {
            unimplemented!();
        }
        Err(_) => 0,
    }
}

pub extern "C" fn _getpwnam(name_ptr: c_int, instance: &mut Instance) -> c_int {
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

    debug!("emscripten::_getpwnam {}", name_ptr);
    let name = unsafe {
        let memory_name_ptr = instance.memory_offset_addr(0, name_ptr as usize) as *const c_char;
        CStr::from_ptr(memory_name_ptr)
    };

    unsafe {
        let passwd = &*libc_getpwnam(name.as_ptr());
        let passwd_struct_offset = (instance.emscripten_data.malloc)(mem::size_of::<GuestPasswd>() as _, instance);

        let passwd_struct_ptr = instance.memory_offset_addr(0, passwd_struct_offset as _) as *mut GuestPasswd;
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
    #[repr(C)]
    struct GuestGroup {
        gr_name: u32,
        gr_passwd: u32,
        gr_gid: u32,
        gr_mem: u32,
    }

    debug!("emscripten::_getgrnam {}", name_ptr);
    let name = unsafe {
        let memory_name_ptr = instance.memory_offset_addr(0, name_ptr as usize) as *const c_char;
        CStr::from_ptr(memory_name_ptr)
    };

    unsafe {
        let group = &*libc_getgrnam(name.as_ptr());
        let group_struct_offset = (instance.emscripten_data.malloc)(mem::size_of::<GuestGroup>() as _, instance);

        let group_struct_ptr = instance.memory_offset_addr(0, group_struct_offset as _) as *mut GuestGroup;
        (*group_struct_ptr).gr_name = copy_cstr_into_wasm(instance, group.gr_name);
        (*group_struct_ptr).gr_passwd = copy_cstr_into_wasm(instance, group.gr_passwd);
        (*group_struct_ptr).gr_gid = group.gr_gid;
        (*group_struct_ptr).gr_mem = copy_terminated_array_of_cstrs(instance, group.gr_mem);

        group_struct_offset as c_int
    }
}
