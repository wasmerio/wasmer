/// NOTE: These syscalls only support wasm_32 for now because they take u32 offset
use libc::{
    c_int, c_long, getenv, getgrnam as libc_getgrnam, getpwnam as libc_getpwnam, putenv, setenv,
    sysconf, unsetenv,
};
use std::ffi::CStr;
use std::mem;
use std::os::raw::c_char;

use super::utils::{allocate_on_stack, copy_cstr_into_wasm, copy_terminated_array_of_cstrs};
use wasmer_runtime::{vm::Ctx, types::{Value, MemoryIndex}, structures::{TypedIndex}};

// #[no_mangle]
/// emscripten: _getenv // (name: *const char) -> *const c_char;
pub extern "C" fn _getenv(name: c_int, vmctx: &mut Ctx) -> u32 {
    debug!("emscripten::_getenv");

    let name_addr = vmctx.memory(MemoryIndex::new(0))[name as usize] as *const c_char;

    debug!("=> name({:?})", unsafe { CStr::from_ptr(name_addr) });

    let c_str = unsafe { getenv(name_addr) };
    if c_str.is_null() {
        return 0;
    }

    unsafe { copy_cstr_into_wasm(vmctx, c_str) }
}

/// emscripten: _setenv // (name: *const char, name: *const value, overwrite: int);
pub extern "C" fn _setenv(name: c_int, value: c_int, overwrite: c_int, vmctx: &mut Ctx) {
    debug!("emscripten::_setenv");

    let name_addr = vmctx.memory(MemoryIndex::new(0))[name as usize] as *const c_char;
    let value_addr = vmctx.memory(MemoryIndex::new(0))[value as usize] as *const c_char;

    debug!("=> name({:?})", unsafe { CStr::from_ptr(name_addr) });
    debug!("=> value({:?})", unsafe { CStr::from_ptr(value_addr) });

    unsafe { setenv(name_addr, value_addr, overwrite) };
}

/// emscripten: _putenv // (name: *const char);
pub extern "C" fn _putenv(name: c_int, vmctx: &mut Ctx) {
    debug!("emscripten::_putenv");

    let name_addr = vmctx.memory(MemoryIndex::new(0))[name as usize] as *const c_char;

    debug!("=> name({:?})", unsafe { CStr::from_ptr(name_addr) });

    unsafe { putenv(name_addr as _) };
}

/// emscripten: _unsetenv // (name: *const char);
pub extern "C" fn _unsetenv(name: c_int, vmctx: &mut Ctx) {
    debug!("emscripten::_unsetenv");

    let name_addr = vmctx.memory(MemoryIndex::new(0))[name as usize] as *const c_char;

    debug!("=> name({:?})", unsafe { CStr::from_ptr(name_addr) });

    unsafe { unsetenv(name_addr) };
}

#[allow(clippy::cast_ptr_alignment)]
pub extern "C" fn _getpwnam(name_ptr: c_int, vmctx: &mut Ctx) -> c_int {
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
        let memory_name_ptr = vmctx.memory(MemoryIndex::new(0))[name_ptr as usize] as *const c_char;
        CStr::from_ptr(memory_name_ptr)
    };

    unsafe {
        let passwd = &*libc_getpwnam(name.as_ptr());
        let passwd_struct_offset = call_malloc(mem::size_of::<GuestPasswd>() as _, vmctx);

        let passwd_struct_ptr =
            vmctx.memory(MemoryIndex::new(0))[passwd_struct_offset as usize] as *mut GuestPasswd;
        (*passwd_struct_ptr).pw_name = copy_cstr_into_wasm(vmctx, passwd.pw_name);
        (*passwd_struct_ptr).pw_passwd = copy_cstr_into_wasm(vmctx, passwd.pw_passwd);
        (*passwd_struct_ptr).pw_gecos = copy_cstr_into_wasm(vmctx, passwd.pw_gecos);
        (*passwd_struct_ptr).pw_dir = copy_cstr_into_wasm(vmctx, passwd.pw_dir);
        (*passwd_struct_ptr).pw_shell = copy_cstr_into_wasm(vmctx, passwd.pw_shell);
        (*passwd_struct_ptr).pw_uid = passwd.pw_uid;
        (*passwd_struct_ptr).pw_gid = passwd.pw_gid;

        passwd_struct_offset as c_int
    }
}

#[allow(clippy::cast_ptr_alignment)]
pub extern "C" fn _getgrnam(name_ptr: c_int, vmctx: &mut Ctx) -> c_int {
    debug!("emscripten::_getgrnam {}", name_ptr);

    #[repr(C)]
    struct GuestGroup {
        gr_name: u32,
        gr_passwd: u32,
        gr_gid: u32,
        gr_mem: u32,
    }

    let name = unsafe {
        let memory_name_ptr = vmctx.memory(MemoryIndex::new(0))[name_ptr as usize] as *const c_char;
        CStr::from_ptr(memory_name_ptr)
    };

    unsafe {
        let group = &*libc_getgrnam(name.as_ptr());
        let group_struct_offset = call_malloc(mem::size_of::<GuestGroup>() as _, vmctx);

        let group_struct_ptr =
            vmctx.memory(MemoryIndex::new(0))[group_struct_offset as usize] as *mut GuestGroup;
        (*group_struct_ptr).gr_name = copy_cstr_into_wasm(vmctx, group.gr_name);
        (*group_struct_ptr).gr_passwd = copy_cstr_into_wasm(vmctx, group.gr_passwd);
        (*group_struct_ptr).gr_gid = group.gr_gid;
        (*group_struct_ptr).gr_mem = copy_terminated_array_of_cstrs(vmctx, group.gr_mem);

        group_struct_offset as c_int
    }
}

pub fn call_malloc(size: i32, vmctx: &mut Ctx) -> u32 {
    let ret = call(vmctx, "_malloc", &[Value::I32(size)])
        .expect("_malloc call failed");

    if let Some(Value::I32(ptr)) = ret {
        ptr as u32
    } else {
        panic!("unexpected value from _malloc: {:?}", ret);
    }
}

pub fn call_memalign(alignment: u32, size: u32, vmctx: &mut Ctx) -> u32 {
    let ret =
        call(
            vmctx,
            "_memalign",
            &[Value::I32(alignment as i32), Value::I32(size as i32)],
        )
        .expect("_memalign call failed");

    if let Some(Value::I32(res)) = ret {
        res as u32
    } else {
        panic!("unexpected value from _memalign {:?}", ret);
    }
}

pub fn call_memset(pointer: u32, value: i32, size: u32, vmctx: &mut Ctx) -> u32 {
    let ret =
        call(
            vmctx,
            "_memset",
            &[
                Value::I32(pointer as i32),
                Value::I32(value),
                Value::I32(size as i32),
            ],
        )
        .expect("_memset call failed");

    if let Some(Value::I32(res)) = ret {
        res as u32
    } else {
        panic!("unexpected value from _memset {:?}", ret);
    }
}

pub extern "C" fn _getpagesize() -> u32 {
    debug!("emscripten::_getpagesize");
    16384
}

#[allow(clippy::cast_ptr_alignment)]
pub extern "C" fn ___build_environment(environ: c_int, vmctx: &mut Ctx) {
    debug!("emscripten::___build_environment {}", environ);
    const MAX_ENV_VALUES: u32 = 64;
    const TOTAL_ENV_SIZE: u32 = 1024;
    let mut environment = vmctx.memory(MemoryIndex::new(0))[environ as usize] as *mut c_int;
    unsafe {
        let (pool_offset, _pool_slice): (u32, &mut [u8]) =
            allocate_on_stack(TOTAL_ENV_SIZE as u32, vmctx);
        let (env_offset, _env_slice): (u32, &mut [u8]) =
            allocate_on_stack((MAX_ENV_VALUES * 4) as u32, vmctx);
        let mut env_ptr = vmctx.memory(MemoryIndex::new(0))[env_offset as usize] as *mut c_int;
        let mut _pool_ptr = vmctx.memory(MemoryIndex::new(0))[pool_offset as usize] as *mut c_int;
        *env_ptr = pool_offset as i32;
        *environment = env_offset as i32;

        // *env_ptr = 0;
    };
    // unsafe {
    //     *env_ptr = 0;
    // };
}

pub extern "C" fn _sysconf(name: c_int, _vmctx: &mut Ctx) -> c_long {
    debug!("emscripten::_sysconf {}", name);
    // TODO: Implement like emscripten expects regarding memory/page size
    unsafe { sysconf(name) }
}

use libffi::high::{arg as libffi_arg, call as libffi_call, CodePtr};
use std::iter;
use wasmer_runtime::{
    error::{CallResult, CallError},
    types::{Type, LocalOrImport, FuncIndex, FuncSig},
    export::{FuncPointer, Context},
    module::ExportIndex,
    recovery::call_protected,
};

/// TODO: May need to implement some sort of trampoline or caching or rethinking of the
/// the implementation as a whole.
/// Calls an exported function from within the vm context.
pub fn call(vmctx: &Ctx, name: &str, args: &[Value]) -> CallResult<Option<Value>> {
    let module = unsafe { &*vmctx.module };
    let export_index = module
            .exports
            .get(name)
            .ok_or_else(|| CallError::NoSuchExport {
                name: name.to_string(),
            })?;

    let func_index = if let ExportIndex::Func(func_index) = export_index {
        *func_index
    } else {
        return Err(CallError::ExportNotFunc {
            name: name.to_string(),
        }
        .into());
    };

    call_with_index(vmctx, func_index, args)
}

/// Call an exported function in vmctx by its index.
fn call_with_index(
    vmctx: &Ctx,
    func_index: FuncIndex,
    args: &[Value],
) -> CallResult<Option<Value>> {
    let (func_ref, _, signature) = get_func_from_index(vmctx, func_index);

    let func_ptr = CodePtr::from_ptr(func_ref.inner() as _);

    assert!(
        signature.returns.len() <= 1,
        "multi-value returns not yet supported"
    );

    if !signature.check_sig(args) {
        Err(CallError::Signature {
            expected: signature.clone(),
            found: args.iter().map(|val| val.ty()).collect(),
        })?
    }

    let vmctx = unsafe {
        std::mem::transmute::<&Ctx, *mut Ctx>(vmctx)
    };

    let libffi_args: Vec<_> = args
        .iter()
        .map(|val| match val {
            Value::I32(ref x) => libffi_arg(x),
            Value::I64(ref x) => libffi_arg(x),
            Value::F32(ref x) => libffi_arg(x),
            Value::F64(ref x) => libffi_arg(x),
        })
        .chain(iter::once(libffi_arg(&vmctx)))
        .collect();

    Ok(call_protected(|| {
        signature
            .returns
            .first()
            .map(|ty| match ty {
                Type::I32 => Value::I32(unsafe { libffi_call(func_ptr, &libffi_args) }),
                Type::I64 => Value::I64(unsafe { libffi_call(func_ptr, &libffi_args) }),
                Type::F32 => Value::F32(unsafe { libffi_call(func_ptr, &libffi_args) }),
                Type::F64 => Value::F64(unsafe { libffi_call(func_ptr, &libffi_args) }),
            })
            .or_else(|| {
                // call with no returns
                unsafe {
                    libffi_call::<()>(func_ptr, &libffi_args);
                }
                None
            })
    })?)
}

/// Get function details by its index.
fn get_func_from_index(
    vmctx: &Ctx,
    func_index: FuncIndex,
) -> (FuncPointer, Context, FuncSig) {
    let module = unsafe { &*vmctx.module };

    let sig_index = *module
        .func_assoc
        .get(func_index)
        .expect("broken invariant, incorrect func index");

    let (func_ptr, ctx) = match func_index.local_or_import(module) {
        LocalOrImport::Local(local_func_index) => (
            module
                .func_resolver
                .get(&module, local_func_index)
                .expect("broken invariant, func resolver not synced with module.exports")
                .cast()
                .as_ptr() as *const _,
            Context::Internal,
        ),
        LocalOrImport::Import(imported_func_index) => {
            let imported_func = unsafe { &(*vmctx.import_backing).functions[imported_func_index] };
            (
                imported_func.func as *const _,
                Context::External(imported_func.vmctx),
            )
        }
    };

    let signature = module.sig_registry.lookup_func_sig(sig_index).clone();

    (unsafe { FuncPointer::new(func_ptr) }, ctx, signature)
}
