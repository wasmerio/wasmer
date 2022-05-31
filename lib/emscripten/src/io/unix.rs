use super::super::env::call_malloc;
use super::super::utils::copy_cstr_into_wasm;
use libc::{chroot as _chroot, getpwuid as _getpwuid, printf as _printf};
use std::mem;

use crate::EmEnv;
use wasmer::{AsContextMut, ContextMut};

/// putchar
pub fn putchar(_ctx: ContextMut<'_, EmEnv>, chr: i32) {
    unsafe { libc::putchar(chr) };
}

/// printf
pub fn printf(ctx: ContextMut<'_ , EmEnv>, memory_offset: i32, extra: i32) -> i32 {
    debug!("emscripten::printf {}, {}", memory_offset, extra);
    unsafe {
        let addr = emscripten_memory_pointer!(ctx, ctx.data().memory(0), memory_offset) as _;
        _printf(addr, extra)
    }
}

/// chroot
pub fn chroot(ctx: ContextMut<'_ , EmEnv>, name_ptr: i32) -> i32 {
    debug!("emscripten::chroot");
    let name = emscripten_memory_pointer!(ctx, ctx.data().memory(0), name_ptr) as *const i8;
    unsafe { _chroot(name as *const _) }
}

/// getpwuid
#[allow(clippy::cast_ptr_alignment)]
pub fn getpwuid(mut ctx: ContextMut<'_ , EmEnv>, uid: i32) -> i32 {
    debug!("emscripten::getpwuid {}", uid);

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

    unsafe {
        let passwd = &*_getpwuid(uid as _);
        let passwd_struct_offset = call_malloc(ctx.as_context_mut(), mem::size_of::<GuestPasswd>() as _);
        let passwd_struct_ptr =
            emscripten_memory_pointer!(ctx, ctx.data().memory(0), passwd_struct_offset) as *mut GuestPasswd;
        assert_eq!(
            passwd_struct_ptr as usize % std::mem::align_of::<GuestPasswd>(),
            0
        );
        (*passwd_struct_ptr).pw_name = copy_cstr_into_wasm(ctx.as_context_mut(), passwd.pw_name);
        (*passwd_struct_ptr).pw_passwd = copy_cstr_into_wasm(ctx.as_context_mut(), passwd.pw_passwd);
        (*passwd_struct_ptr).pw_gecos = copy_cstr_into_wasm(ctx.as_context_mut(), passwd.pw_gecos);
        (*passwd_struct_ptr).pw_dir = copy_cstr_into_wasm(ctx.as_context_mut(), passwd.pw_dir);
        (*passwd_struct_ptr).pw_shell = copy_cstr_into_wasm(ctx.as_context_mut(), passwd.pw_shell);
        (*passwd_struct_ptr).pw_uid = passwd.pw_uid;
        (*passwd_struct_ptr).pw_gid = passwd.pw_gid;

        passwd_struct_offset as _
    }
    // unsafe { _getpwuid(uid as _) as _}
    // 0
}
