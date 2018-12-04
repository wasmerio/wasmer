// use super::varargs::VarArgs;
use crate::webassembly::Instance;

pub extern "C" fn _sigemptyset(set: u32, instance: &mut Instance) -> i32 {
    debug!("emscripten::_sigemptyset");
    let set_addr = instance.memory_offset_addr(0, set as _) as *mut u32;
    unsafe {
        *set_addr = 0;
    }
    0
}

pub extern "C" fn _sigaction(_signum: u32, _act: u32, _oldact: u32, _instance: &mut Instance) -> i32 {
    debug!("emscripten::_sigaction");
    0
}

pub extern "C" fn _sigaddset(set: u32, signum: u32, instance: &mut Instance) -> i32 {
    debug!("emscripten::_sigaddset");
    let set_addr = instance.memory_offset_addr(0, set as _) as *mut u32;
    unsafe {
        *set_addr |= 1 << (signum - 1);
    }
    0
}

pub extern "C" fn _sigprocmask() -> i32 {
    debug!("emscripten::_sigprocmask");
    0
}

pub extern "C" fn _signal(sig: u32, _instance: &mut Instance) -> i32 {
    debug!("emscripten::_signal ({})", sig);
    0
}
