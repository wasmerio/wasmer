use libc::{abort, c_char, c_int, exit, EAGAIN};

#[cfg(not(target_os = "windows"))]
use libc::pid_t;

#[cfg(target_os = "windows")]
type pid_t = c_int;

use std::ffi::CStr;
use wasmer_runtime_core::vm::Ctx;

pub fn abort_with_message(ctx: &mut Ctx, message: &str) {
    debug!("emscripten::abort_with_message");
    println!("{}", message);
    _abort(ctx);
}

pub fn _abort(_ctx: &mut Ctx) {
    debug!("emscripten::_abort");
    unsafe {
        abort();
    }
}

pub fn _fork(_ctx: &mut Ctx) -> pid_t {
    debug!("emscripten::_fork");
    // unsafe {
    //     fork()
    // }
    -1
}

pub fn _endgrent(_ctx: &mut Ctx) {
    debug!("emscripten::_endgrent");
}

pub fn _execve(_ctx: &mut Ctx, _one: i32, _two: i32, _three: i32) -> i32 {
    debug!("emscripten::_execve");
    -1
}

#[allow(unreachable_code)]
pub fn _exit(_ctx: &mut Ctx, status: c_int) {
    // -> !
    debug!("emscripten::_exit {}", status);
    unsafe { exit(status) }
}

pub fn em_abort(ctx: &mut Ctx, message: u32) {
    debug!("emscripten::em_abort {}", message);
    let message_addr = emscripten_memory_pointer!(ctx.memory(0), message) as *mut c_char;
    unsafe {
        let message = CStr::from_ptr(message_addr)
            .to_str()
            .unwrap_or("Unexpected abort");

        abort_with_message(ctx, message);
    }
}

pub fn _kill(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::_kill");
    -1
}

pub fn _sched_yield(_ctx: &mut Ctx) -> i32 {
    debug!("emscripten::_sched_yield");
    -1
}

pub fn _llvm_stacksave(_ctx: &mut Ctx) -> i32 {
    debug!("emscripten::_llvm_stacksave");
    -1
}

pub fn _llvm_stackrestore(_ctx: &mut Ctx, _one: i32) {
    debug!("emscripten::_llvm_stackrestore");
}

pub fn _raise(_ctx: &mut Ctx, _one: i32) -> i32 {
    debug!("emscripten::_raise");
    -1
}

pub fn _sem_init(_ctx: &mut Ctx, _one: i32, _two: i32, _three: i32) -> i32 {
    debug!("emscripten::_sem_init");
    -1
}

pub fn _sem_post(_ctx: &mut Ctx, _one: i32) -> i32 {
    debug!("emscripten::_sem_post");
    -1
}

pub fn _sem_wait(_ctx: &mut Ctx, _one: i32) -> i32 {
    debug!("emscripten::_sem_post");
    -1
}

#[allow(clippy::cast_ptr_alignment)]
pub fn _getgrent(ctx: &mut Ctx) -> c_int {
    debug!("emscripten::_getgrent");
    -1
}

pub fn _setgrent(_ctx: &mut Ctx) {
    debug!("emscripten::_setgrent");
}

pub fn _setgroups(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::_setgroups");
    -1
}

pub fn _setitimer(_ctx: &mut Ctx, _one: i32, _two: i32, _three: i32) -> i32 {
    debug!("emscripten::_setitimer");
    -1
}

pub fn _usleep(_ctx: &mut Ctx, _one: i32) -> i32 {
    debug!("emscripten::_usleep");
    -1
}

pub fn _utimes(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::_utimes");
    -1
}

pub fn _waitpid(_ctx: &mut Ctx, _one: i32, _two: i32, _three: i32) -> i32 {
    debug!("emscripten::_waitpid");
    -1
}

pub fn abort_stack_overflow(ctx: &mut Ctx, _what: c_int) {
    debug!("emscripten::abort_stack_overflow");
    // TODO: Message incomplete. Need to finish em runtime data first
    abort_with_message(
        ctx,
        "Stack overflow! Attempted to allocate some bytes on the stack",
    );
}

pub fn _llvm_trap(ctx: &mut Ctx) {
    debug!("emscripten::_llvm_trap");
    abort_with_message(ctx, "abort!");
}

pub fn _system(_ctx: &mut Ctx, _one: i32) -> c_int {
    debug!("emscripten::_system");
    // TODO: May need to change this Em impl to a working version
    eprintln!("Can't call external programs");
    return EAGAIN;
}

pub fn _popen(_ctx: &mut Ctx, _one: i32, _two: i32) -> c_int {
    debug!("emscripten::_popen");
    // TODO: May need to change this Em impl to a working version
    eprintln!("Missing function: popen");
    unsafe {
        abort();
    }
}
