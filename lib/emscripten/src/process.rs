use libc::{abort, c_char, c_int, exit, pid_t, EAGAIN};

use std::ffi::CStr;
use wasmer_runtime_core::vm::Ctx;

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

pub extern "C" fn _fork(_vmctx: &mut Ctx) -> pid_t {
    debug!("emscripten::_fork");
    // unsafe {
    //     fork()
    // }
    -1
}

pub extern "C" fn _exit(status: c_int, _vmctx: &mut Ctx) -> ! {
    debug!("emscripten::_exit {}", status);
    unsafe { exit(status) }
}

pub extern "C" fn em_abort(message: u32, vmctx: &mut Ctx) {
    debug!("emscripten::em_abort {}", message);
    let message_addr = vmctx.memory(0)[message as usize] as *mut c_char;
    unsafe {
        let message = CStr::from_ptr(message_addr)
            .to_str()
            .unwrap_or("Unexpected abort");

        abort_with_message(message);
    }
}

pub extern "C" fn abort_stack_overflow(what: c_int) {
    debug!("emscripten::abort_stack_overflow");
    // TODO: Message incomplete. Need to finish em runtime data first
    abort_with_message("Stack overflow! Attempted to allocate some bytes on the stack");
}

pub extern "C" fn _llvm_trap() {
    debug!("emscripten::_llvm_trap");
    abort_with_message("abort!");
}

pub extern "C" fn _system() -> c_int {
    debug!("emscripten::_system");
    // TODO: May need to change this Em impl to a working version
    eprintln!("Can't call external programs");
    return EAGAIN;
}

pub extern "C" fn _popen() -> c_int {
    debug!("emscripten::_popen");
    // TODO: May need to change this Em impl to a working version
    eprintln!("Missing function: popen");
    unsafe {
        abort();
    }
}
