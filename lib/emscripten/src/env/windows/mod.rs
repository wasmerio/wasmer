/// NOTE: These syscalls only support wasm_32 for now because they take u32 offset
use libc::{
    c_int,
    c_long,
    getenv,
    //sysconf, unsetenv,
};

use core::slice;
use std::ffi::{CStr, CString};
use std::mem;
use std::os::raw::c_char;

use crate::utils::{
    allocate_on_stack, copy_cstr_into_wasm, copy_terminated_array_of_cstrs, read_string_from_wasm,
};
use crate::EmscriptenData;
use wasmer_runtime_core::memory::Memory;
use wasmer_runtime_core::vm::Ctx;

#[link(name = "c")]
extern "C" {
    #[link_name = "_putenv"]
    pub fn putenv(s: *const c_char) -> c_int;
}

pub fn _getaddrinfo(_one: i32, _two: i32, _three: i32, _four: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::_getaddrinfo");
    -1
}

// #[no_mangle]
/// emscripten: _getenv // (name: *const char) -> *const c_char;
pub fn _getenv(name: i32, ctx: &mut Ctx) -> u32 {
    debug!("emscripten::_getenv");

    let memory = ctx.memory(0);

    let name_addr = emscripten_memory_pointer!(ctx.memory(0), name) as _;

    debug!("=> name({:?})", unsafe { CStr::from_ptr(name_addr) });

    let c_str: *mut c_char = unsafe { getenv(name_addr as _) } as _;
    if c_str.is_null() {
        return 0;
    }

    unsafe { copy_cstr_into_wasm(ctx, c_str) }
}

/// emscripten: _setenv // (name: *const char, name: *const value, overwrite: int);
pub fn _setenv(name: u32, value: u32, overwrite: u32, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::_setenv");
    let name_addr = emscripten_memory_pointer!(ctx.memory(0), name);
    let value_addr = emscripten_memory_pointer!(ctx.memory(0), value);
    // setenv does not exist on windows, so we hack it with _putenv
    let name = read_string_from_wasm(ctx.memory(0), name);
    let value = read_string_from_wasm(ctx.memory(0), value);
    let putenv_string = format!("{}={}", name, value);
    let putenv_cstring = CString::new(putenv_string).unwrap();
    let putenv_raw_ptr = putenv_cstring.as_ptr();
    debug!("=> name({:?})", name);
    debug!("=> value({:?})", value);
    unsafe { putenv(putenv_raw_ptr) }
}

/// emscripten: _putenv // (name: *const char);
pub fn _putenv(name: c_int, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::_putenv");

    let name_addr = emscripten_memory_pointer!(ctx.memory(0), name) as *const c_char;

    debug!("=> name({:?})", unsafe { CStr::from_ptr(name_addr) });
    unsafe { putenv(name_addr) }
}

/// emscripten: _unsetenv // (name: *const char);
pub fn _unsetenv(name: c_int, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::_unsetenv");

    let name_addr = emscripten_memory_pointer!(ctx.memory(0), name);

    debug!("=> name({:?})", unsafe { CStr::from_ptr(name_addr) });

    unsafe { unsetenv(name_addr) }
}

#[allow(clippy::cast_ptr_alignment)]
pub fn _getpwnam(name_ptr: c_int, ctx: &mut Ctx) -> c_int {
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
        let passwd_struct_offset = call_malloc(mem::size_of::<GuestPasswd>() as _, ctx);

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
pub fn _getgrnam(name_ptr: c_int, ctx: &mut Ctx) -> c_int {
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
        let group_struct_offset = call_malloc(mem::size_of::<GuestGroup>() as _, ctx);

        let group_struct_ptr =
            emscripten_memory_pointer!(ctx.memory(0), group_struct_offset) as *mut GuestGroup;
        (*group_struct_ptr).gr_name = copy_cstr_into_wasm(ctx, group.gr_name);
        (*group_struct_ptr).gr_passwd = copy_cstr_into_wasm(ctx, group.gr_passwd);
        (*group_struct_ptr).gr_gid = group.gr_gid;
        (*group_struct_ptr).gr_mem = copy_terminated_array_of_cstrs(ctx, group.gr_mem);

        group_struct_offset as c_int
    }
}

pub fn call_malloc(size: u32, ctx: &mut Ctx) -> u32 {
    get_emscripten_data(ctx).malloc.call(size).unwrap()
}

pub fn call_memalign(alignment: u32, size: u32, ctx: &mut Ctx) -> u32 {
    get_emscripten_data(ctx)
        .memalign
        .call(alignment, size)
        .unwrap()
}

pub fn call_memset(pointer: u32, value: u32, size: u32, ctx: &mut Ctx) -> u32 {
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

#[allow(clippy::cast_ptr_alignment)]
pub fn ___build_environment(environ: c_int, ctx: &mut Ctx) {
    debug!("emscripten::___build_environment {}", environ);
    const MAX_ENV_VALUES: u32 = 64;
    const TOTAL_ENV_SIZE: u32 = 1024;
    let environment = emscripten_memory_pointer!(ctx.memory(0), environ) as *mut c_int;
    unsafe {
        let (pool_offset, _pool_slice): (u32, &mut [u8]) =
            allocate_on_stack(TOTAL_ENV_SIZE as u32, ctx);
        let (env_offset, _env_slice): (u32, &mut [u8]) =
            allocate_on_stack((MAX_ENV_VALUES * 4) as u32, ctx);
        let env_ptr = emscripten_memory_pointer!(ctx.memory(0), env_offset) as *mut c_int;
        let mut _pool_ptr = emscripten_memory_pointer!(ctx.memory(0), pool_offset) as *mut c_int;
        *env_ptr = pool_offset as i32;
        *environment = env_offset as i32;

        // *env_ptr = 0;
    };
    // unsafe {
    //     *env_ptr = 0;
    // };
}

pub fn _sysconf(name: c_int, _ctx: &mut Ctx) -> c_long {
    debug!("emscripten::_sysconf {}", name);
    // TODO: Implement like emscripten expects regarding memory/page size
    unsafe { sysconf(name) }
}

pub fn ___assert_fail(a: c_int, b: c_int, c: c_int, d: c_int, _ctx: &mut Ctx) {
    debug!("emscripten::___assert_fail {} {} {} {}", a, b, c, d);
    // TODO: Implement like emscripten expects regarding memory/page size
    // TODO raise an error
}
