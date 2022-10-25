use super::env;
use super::env::{get_emscripten_data, get_emscripten_funcs};
use crate::storage::align_memory;
use crate::EmEnv;
use libc::stat;
use std::ffi::CStr;
use std::mem::size_of;
use std::os::raw::c_char;
use std::path::PathBuf;
use std::slice;
use wasmer::{FunctionEnvMut, GlobalInit, MemoryView, Module, Pages, WasmPtr};

/// We check if a provided module is an Emscripten generated one
pub fn is_emscripten_module(module: &Module) -> bool {
    for import in module.imports().functions() {
        let name = import.name();
        let module = import.module();
        if (name == "_emscripten_memcpy_big"
            || name == "emscripten_memcpy_big"
            || name == "__map_file")
            && module == "env"
        {
            return true;
        }
    }
    false
}

pub fn get_emscripten_table_size(module: &Module) -> Result<(u32, Option<u32>), String> {
    if let Some(import) = module.imports().tables().next() {
        let ty = import.ty();
        Ok((ty.minimum, ty.maximum))
    } else {
        Err("Emscripten requires at least one imported table".to_string())
    }
}

pub fn get_emscripten_memory_size(module: &Module) -> Result<(Pages, Option<Pages>, bool), String> {
    if let Some(import) = module.imports().memories().next() {
        let ty = import.ty();
        Ok((ty.minimum, ty.maximum, ty.shared))
    } else {
        Err("Emscripten requires at least one imported memory".to_string())
    }
}

/// Reads values written by `-s EMIT_EMSCRIPTEN_METADATA=1`
/// Assumes values start from the end in this order:
/// Last export: Dynamic Base
/// Second-to-Last export: Dynamic top pointer
pub fn get_emscripten_metadata(module: &Module) -> Result<Option<(u32, u32)>, String> {
    let max_idx = match module
        .info()
        .global_initializers
        .iter()
        .map(|(k, _)| k)
        .max()
    {
        Some(x) => x,
        None => return Ok(None),
    };

    let snd_max_idx = match module
        .info()
        .global_initializers
        .iter()
        .map(|(k, _)| k)
        .filter(|k| *k != max_idx)
        .max()
    {
        Some(x) => x,
        None => return Ok(None),
    };

    if let (GlobalInit::I32Const(dynamic_base), GlobalInit::I32Const(dynamictop_ptr)) = (
        &module.info().global_initializers[max_idx],
        &module.info().global_initializers[snd_max_idx],
    ) {
        let dynamic_base = (*dynamic_base as u32).checked_sub(32).ok_or_else(|| {
            format!(
                "emscripten unexpected dynamic_base {}",
                *dynamic_base as u32
            )
        })?;
        let dynamictop_ptr = (*dynamictop_ptr as u32).checked_sub(32).ok_or_else(|| {
            format!(
                "emscripten unexpected dynamictop_ptr {}",
                *dynamictop_ptr as u32
            )
        })?;
        Ok(Some((
            align_memory(dynamic_base),
            align_memory(dynamictop_ptr),
        )))
    } else {
        Ok(None)
    }
}

pub unsafe fn write_to_buf(
    ctx: FunctionEnvMut<EmEnv>,
    string: *const c_char,
    buf: u32,
    max: u32,
) -> u32 {
    let memory = ctx.data().memory(0);
    let buf_addr = emscripten_memory_pointer!(memory.view(&ctx), buf) as *mut c_char;

    for i in 0..max {
        *buf_addr.add(i as _) = *string.add(i as _);
    }

    buf
}

/// This function expects nullbyte to be appended.
pub unsafe fn copy_cstr_into_wasm(ctx: &mut FunctionEnvMut<EmEnv>, cstr: *const c_char) -> u32 {
    let s = CStr::from_ptr(cstr).to_str().unwrap();
    let cstr_len = s.len();
    let space_offset = env::call_malloc(ctx, (cstr_len as u32) + 1);
    let memory = ctx.data().memory(0);
    let raw_memory = emscripten_memory_pointer!(memory.view(&ctx), space_offset) as *mut c_char;
    let slice = slice::from_raw_parts_mut(raw_memory, cstr_len);

    for (byte, loc) in s.bytes().zip(slice.iter_mut()) {
        *loc = byte as _;
    }

    // TODO: Appending null byte won't work, because there is CStr::from_ptr(cstr)
    //      at the top that crashes when there is no null byte
    *raw_memory.add(cstr_len) = 0;

    space_offset
}

/// # Safety
/// This method is unsafe because it operates directly with the slice of memory represented by the address
pub unsafe fn allocate_on_stack<'a, T: Copy>(
    mut ctx: &mut FunctionEnvMut<'a, EmEnv>,
    count: u32,
) -> (u32, &'a mut [T]) {
    let stack_alloc_ref = get_emscripten_funcs(ctx).stack_alloc_ref().unwrap().clone();
    let offset = stack_alloc_ref
        .call(&mut ctx, count * (size_of::<T>() as u32))
        .unwrap();

    let memory = ctx.data().memory(0);
    let addr = emscripten_memory_pointer!(memory.view(&ctx), offset) as *mut T;
    let slice = slice::from_raw_parts_mut(addr, count as usize);

    (offset, slice)
}

/// # Safety
/// This method is unsafe because it uses `allocate_on_stack` which is unsafe
pub unsafe fn allocate_cstr_on_stack<'a>(
    ctx: &'a mut FunctionEnvMut<'a, EmEnv>,
    s: &str,
) -> (u32, &'a [u8]) {
    let (offset, slice) = allocate_on_stack(ctx, (s.len() + 1) as u32);

    use std::iter;
    for (byte, loc) in s.bytes().chain(iter::once(0)).zip(slice.iter_mut()) {
        *loc = byte;
    }

    (offset, slice)
}

#[cfg(not(target_os = "windows"))]
pub unsafe fn copy_terminated_array_of_cstrs(
    mut _ctx: FunctionEnvMut<EmEnv>,
    cstrs: *mut *mut c_char,
) -> u32 {
    let _total_num = {
        let mut ptr = cstrs;
        let mut counter = 0;
        while !(*ptr).is_null() {
            counter += 1;
            ptr = ptr.add(1);
        }
        counter
    };
    debug!(
        "emscripten::copy_terminated_array_of_cstrs::total_num: {}",
        _total_num
    );
    0
}

#[repr(C)]
pub struct GuestStat {
    st_dev: u32,
    __st_dev_padding: u32,
    __st_ino_truncated: u32,
    st_mode: u32,
    st_nlink: u32,
    st_uid: u32,
    st_gid: u32,
    st_rdev: u32,
    __st_rdev_padding: u32,
    st_size: u32,
    st_blksize: u32,
    st_blocks: u32,
    st_atime: u64,
    st_mtime: u64,
    st_ctime: u64,
    st_ino: u32,
}

#[allow(clippy::cast_ptr_alignment)]
pub unsafe fn copy_stat_into_wasm(ctx: FunctionEnvMut<EmEnv>, buf: u32, stat: &stat) {
    let memory = ctx.data().memory(0);
    let stat_ptr = emscripten_memory_pointer!(memory.view(&ctx), buf) as *mut GuestStat;
    (*stat_ptr).st_dev = stat.st_dev as _;
    (*stat_ptr).__st_dev_padding = 0;
    (*stat_ptr).__st_ino_truncated = stat.st_ino as _;
    (*stat_ptr).st_mode = stat.st_mode as _;
    (*stat_ptr).st_nlink = stat.st_nlink as _;
    (*stat_ptr).st_uid = stat.st_uid as _;
    (*stat_ptr).st_gid = stat.st_gid as _;
    (*stat_ptr).st_rdev = stat.st_rdev as _;
    (*stat_ptr).__st_rdev_padding = 0;
    (*stat_ptr).st_size = stat.st_size as _;
    (*stat_ptr).st_blksize = 4096;
    #[cfg(not(target_os = "windows"))]
    {
        (*stat_ptr).st_blocks = stat.st_blocks as _;
    }
    #[cfg(target_os = "windows")]
    {
        (*stat_ptr).st_blocks = 0;
    }
    (*stat_ptr).st_atime = stat.st_atime as _;
    (*stat_ptr).st_mtime = stat.st_mtime as _;
    (*stat_ptr).st_ctime = stat.st_ctime as _;
    (*stat_ptr).st_ino = stat.st_ino as _;
}

#[allow(dead_code)] // it's used in `env/windows/mod.rs`.
pub fn read_string_from_wasm(memory: &MemoryView, offset: u32) -> String {
    WasmPtr::<u8>::new(offset)
        .read_utf8_string_with_nul(memory)
        .unwrap()
}

/// This function trys to find an entry in mapdir
/// translating paths into their correct value
pub fn get_cstr_path(ctx: FunctionEnvMut<EmEnv>, path: *const i8) -> Option<std::ffi::CString> {
    use std::collections::VecDeque;

    let path_str =
        unsafe { std::ffi::CStr::from_ptr(path as *const _).to_str().unwrap() }.to_string();
    let data = get_emscripten_data(&ctx);
    let path = PathBuf::from(path_str);
    let mut prefix_added = false;
    let mut components = path.components().collect::<VecDeque<_>>();
    // TODO(mark): handle absolute/non-canonical/non-relative paths too (this
    // functionality should be shared among the abis)
    if components.len() == 1 {
        components.push_front(std::path::Component::CurDir);
        prefix_added = true;
    }
    let mut cumulative_path = PathBuf::new();
    for c in components.into_iter() {
        cumulative_path.push(c);
        if let Some(val) = data
            .as_ref()
            .unwrap()
            .mapped_dirs
            .get(&cumulative_path.to_string_lossy().to_string())
        {
            let rest_of_path = if !prefix_added {
                path.strip_prefix(cumulative_path).ok()?
            } else {
                &path
            };
            let rebased_path = val.join(rest_of_path);
            return std::ffi::CString::new(rebased_path.to_string_lossy().as_bytes()).ok();
        }
    }
    None
}

/// gets the current directory
/// handles mapdir logic
pub fn get_current_directory(ctx: FunctionEnvMut<EmEnv>) -> Option<PathBuf> {
    if let Some(val) = get_emscripten_data(&ctx)
        .as_ref()
        .unwrap()
        .mapped_dirs
        .get(".")
    {
        return Some(val.clone());
    }
    std::env::current_dir()
        .map(|cwd| {
            if let Some(val) = get_emscripten_data(&ctx)
                .as_ref()
                .unwrap()
                .mapped_dirs
                .get(&cwd.to_string_lossy().to_string())
            {
                val.clone()
            } else {
                cwd
            }
        })
        .ok()
}
