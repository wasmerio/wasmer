/// NOTE: These syscalls only support wasm_32 for now because they take u32 offset

use libc::{
    c_int,
    c_void,
    size_t,
    ssize_t,
    exit,
    read,
    open,
    close,
};
use std::os::raw::c_char;
use std::ffi::CStr;

use crate::webassembly::{Instance};

/// emscripten: _getenv
pub extern "C" fn get_env(name_ptr: c_int, instance: &mut Instance) -> c_int {
    //   name = Pointer_stringify(name);
    //   if (!ENV.hasOwnProperty(name)) return 0;
    //   if (_getenv.ret) _free(_getenv.ret);
    //   _getenv.ret = allocate(intArrayFromString(ENV[name]), "i8", ALLOC_NORMAL);
    //   return _getenv.ret;
    debug!("host::get_env {}", name_ptr);
    let name = unsafe {
        let memory_name_ptr = instance.memory_offset_addr(0, name_ptr as usize) as *const c_char;
        CStr::from_ptr(memory_name_ptr)
    };
    // let x: &str; unsafe { mem::transmute(str_addr) }
    debug!("host::get_env::name {:?}", name);
    return 0
}
