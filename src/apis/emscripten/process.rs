use libc::{abort, c_char, pid_t, c_int, fork, exit};

use crate::webassembly::Instance;
use std::ffi::CStr;

pub extern "C" fn abort_with_message(message: &str) {
    debug!("emscripten::abort_with_message");
    println!("{}", message);
    _abort();
}

pub extern "C" fn _abort() {
    debug!("emscripten::_abort");
    unsafe {
        abort();
    }
}

pub extern "C" fn _fork(_instance: &mut Instance) -> pid_t {
    debug!("emscripten::_fork");
    // unsafe {
    //     fork()
    // }
    -1
}

pub extern "C" fn _exit(status: c_int, _instance: &mut Instance) -> ! {
    debug!("emscripten::_exit");
    unsafe {
        exit(status)
    }
}

pub extern "C" fn em_abort(message: u32, instance: &mut Instance) {
    debug!("emscripten::em_abort");
    let message_addr = instance.memory_offset_addr(0, message as usize) as *mut c_char;
    unsafe {
        let message = CStr::from_ptr(message_addr)
            .to_str()
            .unwrap_or("Unexpected abort");

        abort_with_message(message);
    }
}

pub extern "C" fn abort_stack_overflow() {
    debug!("emscripten::abort_stack_overflow");
    // TODO: Message incomplete. Need to finish em runtime data first
    abort_with_message("Stack overflow! Attempted to allocate some bytes on the stack");
}
