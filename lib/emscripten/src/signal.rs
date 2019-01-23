// use super::varargs::VarArgs;
use wasmer_runtime_core::vm::Ctx;

#[allow(clippy::cast_ptr_alignment)]
pub extern "C" fn _sigemptyset(set: u32, vmctx: &mut Ctx) -> i32 {
    debug!("emscripten::_sigemptyset");
    let set_addr = vmctx.memory(0)[set as usize] as *mut u32;
    unsafe {
        *set_addr = 0;
    }
    0
}

pub extern "C" fn _sigaction(signum: u32, act: u32, oldact: u32, _vmctx: &mut Ctx) -> i32 {
    debug!("emscripten::_sigaction {}, {}, {}", signum, act, oldact);
    0
}

#[allow(clippy::cast_ptr_alignment)]
pub extern "C" fn _sigaddset(set: u32, signum: u32, vmctx: &mut Ctx) -> i32 {
    debug!("emscripten::_sigaddset {}, {}", set, signum);
    let set_addr = vmctx.memory(0)[set as usize] as *mut u32;
    unsafe {
        *set_addr |= 1 << (signum - 1);
    }
    0
}

pub extern "C" fn _sigprocmask(_vmctx: &mut Ctx) -> i32 {
    debug!("emscripten::_sigprocmask");
    0
}

pub extern "C" fn _signal(sig: u32, _vmctx: &mut Ctx) -> i32 {
    debug!("emscripten::_signal ({})", sig);
    0
}
