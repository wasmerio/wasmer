use crate::webassembly::module::Module;
use crate::webassembly::Instance;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::{slice, mem};

/// We check if a provided module is an Emscripten generated one
pub fn is_emscripten_module(module: &Module) -> bool {
    for (module, _field) in &module.info.imported_funcs {
        if module == "env" {
            return true;
        }
    }
    return false;
}

pub unsafe fn copy_cstr_into_wasm(instance: &mut Instance, cstr: *const c_char) -> u32 {
    let s = CStr::from_ptr(cstr).to_str().unwrap();
    let space_offset = (instance.emscripten_data.malloc)(s.len() as _, instance);
    let raw_memory = instance.memory_offset_addr(0, space_offset as _) as *mut u8;
    let mut slice = slice::from_raw_parts_mut(raw_memory, s.len());

    for (byte, loc) in s.bytes().zip(slice.iter_mut()) {
        *loc = byte;
    }
    space_offset
}

pub unsafe fn copy_terminated_array_of_cstrs(instance: &mut Instance, cstrs: *mut *mut c_char) -> u32 {
    let total_num = {
        let mut ptr = cstrs;
        let mut counter = 0;
        while !(*ptr).is_null() {
            counter += 1;
            ptr = ptr.add(1);
        }
        counter
    };
    debug!("emscripten::copy_terminated_array_of_cstrs::total_num: {}", total_num);
    0
}

#[cfg(test)]
mod tests {
    use super::super::generate_emscripten_env;
    use super::is_emscripten_module;
    use crate::webassembly::instantiate;

    #[test]
    fn should_detect_emscripten_files() {
        let wasm_bytes = include_wast2wasm_bytes!("tests/is_emscripten_true.wast");
        let import_object = generate_emscripten_env();
        let result_object = instantiate(wasm_bytes, import_object).expect("Not compiled properly");
        assert!(is_emscripten_module(&result_object.module));
    }

    #[test]
    fn should_detect_non_emscripten_files() {
        let wasm_bytes = include_wast2wasm_bytes!("tests/is_emscripten_false.wast");
        let import_object = generate_emscripten_env();
        let result_object = instantiate(wasm_bytes, import_object).expect("Not compiled properly");
        assert!(!is_emscripten_module(&result_object.module));
    }
}
