extern crate libc;
use crate::EmEnv;

#[cfg(unix)]
use std::convert::TryInto;

pub fn current_sigrtmax() -> i32 {
    debug!("emscripten::current_sigrtmax");
    0
}

pub fn current_sigrtmin() -> i32 {
    debug!("emscripten::current_sigrtmin");
    0
}

pub fn endpwent() {
    debug!("emscripten::endpwent");
}

pub fn execv(_a: i32, _b: i32) -> i32 {
    debug!("emscripten::execv");
    0
}

pub fn fexecve(_a: i32, _b: i32, _c: i32) -> i32 {
    debug!("emscripten::fexecve");
    0
}

pub fn fpathconf(_a: i32, _b: i32) -> i32 {
    debug!("emscripten::fpathconf");
    0
}

pub fn getitimer(_a: i32, _b: i32) -> i32 {
    debug!("emscripten::getitimer");
    0
}

pub fn getpwent() -> i32 {
    debug!("emscripten::getpwent");
    0
}

pub fn killpg(_a: i32, _b: i32) -> i32 {
    debug!("emscripten::killpg");
    0
}

#[cfg(unix)]
pub fn pathconf(ctx: &EmEnv, path_ptr: i32, name: i32) -> i32 {
    debug!("emscripten::pathconf");
    let path = emscripten_memory_pointer!(ctx.memory(0), path_ptr) as *const i8;
    unsafe { libc::pathconf(path as *const _, name).try_into().unwrap() }
}

#[cfg(not(unix))]
pub fn pathconf(_ctx: &EmEnv, _path_ptr: i32, _name: i32) -> i32 {
    debug!("emscripten::pathconf");
    0
}

pub fn setpwent() {
    debug!("emscripten::setpwent");
}

pub fn sigismember(_a: i32, _b: i32) -> i32 {
    debug!("emscripten::sigismember");
    0
}

pub fn sigpending(_a: i32) -> i32 {
    debug!("emscripten::sigpending");
    0
}
