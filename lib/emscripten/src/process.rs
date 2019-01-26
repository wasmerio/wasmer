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

pub extern "C" fn _fork(_ctx: &mut Ctx) -> pid_t {
    debug!("emscripten::_fork");
    // unsafe {
    //     fork()
    // }
    -1
}

pub extern "C" fn _endgrent(_ctx: &mut Ctx) {
    debug!("emscripten::_endgrent");
}

pub extern "C" fn _execve(one: i32, two: i32, three: i32, ctx: &mut Ctx) -> i32 {
    debug!("emscripten::_execve");
    -1
}

pub extern "C" fn _exit(status: c_int, _ctx: &mut Ctx) -> ! {
    debug!("emscripten::_exit {}", status);
    unsafe { exit(status) }
}

pub extern "C" fn em_abort(message: u32, ctx: &mut Ctx) {
    debug!("emscripten::em_abort {}", message);
    let message_addr = emscripten_memory_pointer!(ctx.memory(0), message) as *mut c_char;
    unsafe {
        let message = CStr::from_ptr(message_addr)
            .to_str()
            .unwrap_or("Unexpected abort");

        abort_with_message(message);
    }
}

pub extern "C" fn _kill(one: i32, two: i32, ctx: &mut Ctx) -> i32 {
    debug!("emscripten::_kill");
    -1
}

pub extern "C" fn _llvm_stackrestore(one: i32, ctx: &mut Ctx) {
    debug!("emscripten::_llvm_stackrestore");
}

pub extern "C" fn _raise(one: i32, ctx: &mut Ctx) -> i32 {
    debug!("emscripten::_raise");
    -1
}

pub extern "C" fn _sem_init(one: i32, two: i32, three: i32, ctx: &mut Ctx) -> i32 {
    debug!("emscripten::_sem_init");
    -1
}

pub extern "C" fn _sem_post(one: i32, ctx: &mut Ctx) -> i32 {
    debug!("emscripten::_sem_post");
    -1
}

pub extern "C" fn _sem_wait(one: i32, ctx: &mut Ctx) -> i32 {
    debug!("emscripten::_sem_post");
    -1
}

pub extern "C" fn _setgrent(ctx: &mut Ctx) {
    debug!("emscripten::_setgrent");
}

pub extern "C" fn _setgroups(one: i32, two: i32, ctx: &mut Ctx) -> i32 {
    debug!("emscripten::_setgroups");
    -1
}

pub extern "C" fn _setitimer(one: i32, two: i32, three: i32, ctx: &mut Ctx) -> i32 {
    debug!("emscripten::_setitimer");
    -1
}

pub extern "C" fn _usleep(one: i32, ctx: &mut Ctx) -> i32 {
    debug!("emscripten::_usleep");
    -1
}

pub extern "C" fn _utimes(one: i32, two: i32, ctx: &mut Ctx) -> i32 {
    debug!("emscripten::_utimes");
    -1
}

pub extern "C" fn _waitpid(one: i32, two: i32, three: i32, ctx: &mut Ctx) -> i32 {
    debug!("emscripten::_waitpid");
    -1
}

pub extern "C" fn abort_stack_overflow(_what: c_int, _ctx: &mut Ctx) {
    debug!("emscripten::abort_stack_overflow");
    // TODO: Message incomplete. Need to finish em runtime data first
    abort_with_message("Stack overflow! Attempted to allocate some bytes on the stack");
}

pub extern "C" fn _llvm_trap(_ctx: &mut Ctx) {
    debug!("emscripten::_llvm_trap");
    abort_with_message("abort!");
}

pub extern "C" fn _system(one: i32, _ctx: &mut Ctx) -> c_int {
    debug!("emscripten::_system");
    // TODO: May need to change this Em impl to a working version
    eprintln!("Can't call external programs");
    return EAGAIN;
}

pub extern "C" fn _popen(one: i32, two: i32, _ctx: &mut Ctx) -> c_int {
    debug!("emscripten::_popen");
    // TODO: May need to change this Em impl to a working version
    eprintln!("Missing function: popen");
    unsafe {
        abort();
    }
}
