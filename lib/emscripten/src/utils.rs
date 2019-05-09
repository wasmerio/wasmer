use super::env;
use super::env::get_emscripten_data;
use crate::storage::align_memory;
use libc::stat;
use std::ffi::CStr;
use std::mem::size_of;
use std::os::raw::c_char;
use std::slice;
use wasmer_runtime_core::memory::Memory;
use wasmer_runtime_core::{
    module::Module,
    structures::TypedIndex,
    types::{ImportedMemoryIndex, ImportedTableIndex},
    units::Pages,
    vm::Ctx,
};

/// We check if a provided module is an Emscripten generated one
pub fn is_emscripten_module(module: &Module) -> bool {
    for (_, import_name) in &module.info().imported_functions {
        let namespace = module
            .info()
            .namespace_table
            .get(import_name.namespace_index);
        let field = module.info().name_table.get(import_name.name_index);
        if field == "_emscripten_memcpy_big" && namespace == "env" {
            return true;
        }
    }
    false
}

pub fn get_emscripten_table_size(module: &Module) -> (u32, Option<u32>) {
    let (_, table) = &module.info().imported_tables[ImportedTableIndex::new(0)];
    (table.minimum, table.maximum)
}

pub fn get_emscripten_memory_size(module: &Module) -> (Pages, Option<Pages>) {
    let (_, memory) = &module.info().imported_memories[ImportedMemoryIndex::new(0)];
    (memory.minimum, memory.maximum)
}

/// Reads values written by `-s EMIT_EMSCRIPTEN_METADATA=1`
/// Assumes values start from the end in this order:
/// Last export: Dynamic Base
/// Second-to-Last export: Dynamic top pointer
pub fn get_emscripten_metadata(module: &Module) -> Option<(u32, u32)> {
    let max_idx = &module.info().globals.iter().map(|(k, _)| k).max()?;
    let snd_max_idx = &module
        .info()
        .globals
        .iter()
        .map(|(k, _)| k)
        .filter(|k| k != max_idx)
        .max()?;

    use wasmer_runtime_core::types::{GlobalInit, Initializer::Const, Value::I32};
    if let (
        GlobalInit {
            init: Const(I32(dynamic_base)),
            ..
        },
        GlobalInit {
            init: Const(I32(dynamictop_ptr)),
            ..
        },
    ) = (
        &module.info().globals[*max_idx],
        &module.info().globals[*snd_max_idx],
    ) {
        Some((
            align_memory(*dynamic_base as u32 - 32),
            align_memory(*dynamictop_ptr as u32 - 32),
        ))
    } else {
        None
    }
}

pub unsafe fn write_to_buf(ctx: &mut Ctx, string: *const c_char, buf: u32, max: u32) -> u32 {
    let buf_addr = emscripten_memory_pointer!(ctx.memory(0), buf) as *mut c_char;

    for i in 0..max {
        *buf_addr.add(i as _) = *string.add(i as _);
    }

    buf
}

/// This function expects nullbyte to be appended.
pub unsafe fn copy_cstr_into_wasm(ctx: &mut Ctx, cstr: *const c_char) -> u32 {
    let s = CStr::from_ptr(cstr).to_str().unwrap();
    let cstr_len = s.len();
    let space_offset = env::call_malloc(ctx, (cstr_len as u32) + 1);
    let raw_memory = emscripten_memory_pointer!(ctx.memory(0), space_offset) as *mut c_char;
    let slice = slice::from_raw_parts_mut(raw_memory, cstr_len);

    for (byte, loc) in s.bytes().zip(slice.iter_mut()) {
        *loc = byte as _;
    }

    // TODO: Appending null byte won't work, because there is CStr::from_ptr(cstr)
    //      at the top that crashes when there is no null byte
    *raw_memory.add(cstr_len) = 0;

    space_offset
}

pub unsafe fn allocate_on_stack<'a, T: Copy>(ctx: &'a mut Ctx, count: u32) -> (u32, &'a mut [T]) {
    let offset = get_emscripten_data(ctx)
        .stack_alloc
        .call(count * (size_of::<T>() as u32))
        .unwrap();
    let addr = emscripten_memory_pointer!(ctx.memory(0), offset) as *mut T;
    let slice = slice::from_raw_parts_mut(addr, count as usize);

    (offset, slice)
}

pub unsafe fn allocate_cstr_on_stack<'a>(ctx: &'a mut Ctx, s: &str) -> (u32, &'a [u8]) {
    let (offset, slice) = allocate_on_stack(ctx, (s.len() + 1) as u32);

    use std::iter;
    for (byte, loc) in s.bytes().chain(iter::once(0)).zip(slice.iter_mut()) {
        *loc = byte;
    }

    (offset, slice)
}

#[cfg(not(target_os = "windows"))]
pub unsafe fn copy_terminated_array_of_cstrs(_ctx: &mut Ctx, cstrs: *mut *mut c_char) -> u32 {
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
pub unsafe fn copy_stat_into_wasm(ctx: &mut Ctx, buf: u32, stat: &stat) {
    let stat_ptr = emscripten_memory_pointer!(ctx.memory(0), buf) as *mut GuestStat;
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
pub fn read_string_from_wasm(memory: &Memory, offset: u32) -> String {
    let v: Vec<u8> = memory.view()[(offset as usize)..]
        .iter()
        .map(|cell| cell.get())
        .take_while(|&byte| byte != 0)
        .collect();
    String::from_utf8_lossy(&v).to_owned().to_string()
}

#[cfg(test)]
mod tests {
    use super::is_emscripten_module;
    use std::sync::Arc;
    use wabt::wat2wasm;
    use wasmer_runtime_core::backend::Compiler;
    use wasmer_runtime_core::compile_with;

    #[cfg(feature = "clif")]
    fn get_compiler() -> impl Compiler {
        use wasmer_clif_backend::CraneliftCompiler;
        CraneliftCompiler::new()
    }

    #[cfg(feature = "llvm")]
    fn get_compiler() -> impl Compiler {
        use wasmer_llvm_backend::LLVMCompiler;
        LLVMCompiler::new()
    }

    #[cfg(feature = "singlepass")]
    fn get_compiler() -> impl Compiler {
        use wasmer_singlepass_backend::SinglePassCompiler;
        SinglePassCompiler::new()
    }

    #[cfg(not(any(feature = "llvm", feature = "clif", feature = "singlepass")))]
    fn get_compiler() -> impl Compiler {
        panic!("compiler not specified, activate a compiler via features");
        use wasmer_clif_backend::CraneliftCompiler;
        CraneliftCompiler::new()
    }

    #[test]
    fn should_detect_emscripten_files() {
        const WAST_BYTES: &[u8] = include_bytes!("tests/is_emscripten_true.wast");
        let wasm_binary = wat2wasm(WAST_BYTES.to_vec()).expect("Can't convert to wasm");
        let module =
            compile_with(&wasm_binary[..], &get_compiler()).expect("WASM can't be compiled");
        let module = Arc::new(module);
        assert!(is_emscripten_module(&module));
    }

    #[test]
    fn should_detect_non_emscripten_files() {
        const WAST_BYTES: &[u8] = include_bytes!("tests/is_emscripten_false.wast");
        let wasm_binary = wat2wasm(WAST_BYTES.to_vec()).expect("Can't convert to wasm");
        let module =
            compile_with(&wasm_binary[..], &get_compiler()).expect("WASM can't be compiled");
        let module = Arc::new(module);
        assert!(!is_emscripten_module(&module));
    }
}
