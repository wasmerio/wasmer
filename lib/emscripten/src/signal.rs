// use super::varargs::VarArgs;
use crate::EmEnv;

#[allow(clippy::cast_ptr_alignment)]
pub fn _sigemptyset(ctx: &EmEnv, set: u32) -> i32 {
    debug!("emscripten::_sigemptyset");
    let set_addr = emscripten_memory_pointer!(ctx.memory(0), set) as *mut u32;
    unsafe {
        *set_addr = 0;
    }
    0
}

pub fn _sigaction(_ctx: &EmEnv, _signum: u32, _act: u32, _oldact: u32) -> i32 {
    debug!("emscripten::_sigaction {}, {}, {}", _signum, _act, _oldact);
    0
}

pub fn _siginterrupt(_ctx: &EmEnv, _a: u32, _b: u32) -> i32 {
    debug!("emscripten::_siginterrupt {}, {}", _a, _b);
    0
}

#[allow(clippy::cast_ptr_alignment)]
pub fn _sigaddset(ctx: &EmEnv, set: u32, signum: u32) -> i32 {
    debug!("emscripten::_sigaddset {}, {}", set, signum);
    let set_addr = emscripten_memory_pointer!(ctx.memory(0), set) as *mut u32;
    unsafe {
        *set_addr |= 1 << (signum - 1);
    }
    0
}

pub fn _sigsuspend(_ctx: &EmEnv, _one: i32) -> i32 {
    debug!("emscripten::_sigsuspend");
    -1
}

pub fn _sigprocmask(_ctx: &EmEnv, _one: i32, _two: i32, _three: i32) -> i32 {
    debug!("emscripten::_sigprocmask");
    0
}

pub fn _signal(_ctx: &EmEnv, _sig: u32, _two: i32) -> i32 {
    debug!("emscripten::_signal ({})", _sig);
    0
}
