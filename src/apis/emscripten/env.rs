use super::super::host;
/// NOTE: These syscalls only support wasm_32 for now because they take u32 offset
use libc::{c_int};
use std::ffi::CStr;
use std::os::raw::c_char;

use crate::webassembly::Instance;

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
