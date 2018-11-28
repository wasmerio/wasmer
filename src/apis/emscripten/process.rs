use libc::{abort, c_char};

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

pub extern "C" fn _sigemptyset(set: u32, instance: &mut Instance) -> i32 {
    debug!("emscripten::_sigemptyset");
    let set_addr = instance.memory_offset_addr(0, set as _) as *mut u32;
    unsafe {
        *set_addr = 0;
    }
    0
}

pub extern "C" fn _sigaction(_signum: u32, act: u32, oldact: u32) -> i32 {
    debug!("emscripten::_sigaction");
    0
}

pub extern "C" fn _sigaddset(set: u32, signum: u32, instance: &mut Instance) -> i32 {
    let set_addr = instance.memory_offset_addr(0, set as _) as *mut u32;
    unsafe {
        *set_addr |= 1 << (signum - 1);
    }
    0
}

pub extern "C" fn _sigprocmask() -> i32 {
    0
}