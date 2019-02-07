/// NOTE: These syscalls only support wasm_32 for now because they take u32 offset
use libc::{c_int, c_long, getenv};

use std::ffi::{CStr, CString};
use std::mem;
use std::os::raw::c_char;

use crate::utils::{allocate_on_stack, copy_cstr_into_wasm, read_string_from_wasm};
use crate::EmscriptenData;
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
pub fn _getenv(name: u32, ctx: &mut Ctx) -> u32 {
    debug!("emscripten::_getenv");
    let name_string = read_string_from_wasm(ctx.memory(0), name);
    debug!("=> name({:?})", name_string);
    let c_str = unsafe { getenv(name_string.as_ptr() as *const libc::c_char) };
    if c_str.is_null() {
        return 0;
    }
    copy_cstr_into_wasm(ctx, c_str as *const c_char)
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
pub fn _unsetenv(name: u32, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::_unsetenv");
    let name_addr = emscripten_memory_pointer!(ctx.memory(0), name);
    let name = read_string_from_wasm(ctx.memory(0), name);
    // no unsetenv on windows, so use putenv with an empty value
    let unsetenv_string = format!("{}=", name);
    let unsetenv_cstring = CString::new(unsetenv_string).unwrap();
    let unsetenv_raw_ptr = unsetenv_cstring.as_ptr();
    debug!("=> name({:?})", name);
    unsafe { putenv(unsetenv_raw_ptr) }
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

    // stub this in windows as it is not valid
    unsafe {
        let passwd_struct_offset = call_malloc(mem::size_of::<GuestPasswd>() as _, ctx);
        let passwd_struct_ptr =
            emscripten_memory_pointer!(ctx.memory(0), passwd_struct_offset) as *mut GuestPasswd;
        (*passwd_struct_ptr).pw_name = 0;
        (*passwd_struct_ptr).pw_passwd = 0;
        (*passwd_struct_ptr).pw_gecos = 0;
        (*passwd_struct_ptr).pw_dir = 0;
        (*passwd_struct_ptr).pw_shell = 0;
        (*passwd_struct_ptr).pw_uid = 0;
        (*passwd_struct_ptr).pw_gid = 0;

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

    // stub the group struct as it is not supported on windows
    unsafe {
        let group_struct_offset = call_malloc(mem::size_of::<GuestGroup>() as _, ctx);
        let group_struct_ptr =
            emscripten_memory_pointer!(ctx.memory(0), group_struct_offset) as *mut GuestGroup;
        (*group_struct_ptr).gr_name = 0;
        (*group_struct_ptr).gr_passwd = 0;
        (*group_struct_ptr).gr_gid = 0;
        (*group_struct_ptr).gr_mem = 0;
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
    // stub because sysconf is not valid on windows
    0
}

pub fn ___assert_fail(a: c_int, b: c_int, c: c_int, d: c_int, _ctx: &mut Ctx) {
    debug!("emscripten::___assert_fail {} {} {} {}", a, b, c, d);
    // TODO: Implement like emscripten expects regarding memory/page size
    // TODO raise an error
}
