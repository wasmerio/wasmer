use libc::{abort, c_int, exit, EAGAIN};

#[cfg(not(target_os = "windows"))]
type PidT = libc::pid_t;
#[cfg(target_os = "windows")]
type PidT = c_int;

use wasmer_runtime_core::vm::Ctx;

pub fn abort_with_message(ctx: &mut Ctx, message: &str) {
    debug!("emscripten::abort_with_message");
    println!("{}", message);
    _abort(ctx);
}

/// The name of this call is `abort` but we want to avoid conflicts with libc::abort
pub fn em_abort(ctx: &mut Ctx, arg: u32) {
    debug!("emscripten::abort");
    eprintln!("Program aborted with value {}", arg);
    _abort(ctx);
}

pub fn _abort(_ctx: &mut Ctx) {
    debug!("emscripten::_abort");
    unsafe {
        abort();
    }
}

pub fn _prctl(ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::_prctl");
    abort_with_message(ctx, "missing function: prctl");
    -1
}

pub fn _fork(_ctx: &mut Ctx) -> PidT {
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
    debug!("emscripten::_sem_init: {}, {}, {}", _one, _two, _three);
    0
}

pub fn _sem_destroy(_ctx: &mut Ctx, _one: i32) -> i32 {
    debug!("emscripten::_sem_destroy");
    0
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
pub fn _getgrent(_ctx: &mut Ctx) -> c_int {
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

pub fn _nanosleep(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::_nanosleep");
    -1
}

pub fn _utime(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::_utime");
    -1
}

pub fn _utimes(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::_utimes");
    -1
}

pub fn _wait(_ctx: &mut Ctx, _one: i32) -> i32 {
    debug!("emscripten::_wait");
    -1
}

pub fn _wait3(_ctx: &mut Ctx, _one: i32, _two: i32, _three: i32) -> i32 {
    debug!("emscripten::_wait3");
    -1
}

pub fn _wait4(_ctx: &mut Ctx, _one: i32, _two: i32, _three: i32, _d: i32) -> i32 {
    debug!("emscripten::_wait4");
    -1
}

pub fn _waitid(_ctx: &mut Ctx, _one: i32, _two: i32, _three: i32, _d: i32) -> i32 {
    debug!("emscripten::_waitid");
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

pub fn _llvm_eh_typeid_for(_ctx: &mut Ctx, _type_info_addr: u32) -> i32 {
    debug!("emscripten::_llvm_eh_typeid_for");
    -1
}

pub fn _system(_ctx: &mut Ctx, _one: i32) -> c_int {
    debug!("emscripten::_system");
    // TODO: May need to change this Em impl to a working version
    eprintln!("Can't call external programs");
    EAGAIN
}

pub fn _popen(_ctx: &mut Ctx, _one: i32, _two: i32) -> c_int {
    debug!("emscripten::_popen");
    // TODO: May need to change this Em impl to a working version
    eprintln!("Missing function: popen");
    unsafe {
        abort();
    }
}
