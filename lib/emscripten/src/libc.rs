extern crate libc;
use crate::EmEnv;
use wasmer::ContextMut;

#[cfg(unix)]
use std::convert::TryInto;

pub fn current_sigrtmax(mut _ctx: ContextMut<'_, EmEnv>) -> i32 {
    debug!("emscripten::current_sigrtmax");
    0
}

pub fn current_sigrtmin(mut _ctx: ContextMut<'_, EmEnv>) -> i32 {
    debug!("emscripten::current_sigrtmin");
    0
}

pub fn endpwent(mut _ctx: ContextMut<'_, EmEnv>) {
    debug!("emscripten::endpwent");
}

pub fn execv(mut _ctx: ContextMut<'_, EmEnv>, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::execv");
    0
}

pub fn fexecve(mut _ctx: ContextMut<'_, EmEnv>, _a: i32, _b: i32, _c: i32) -> i32 {
    debug!("emscripten::fexecve");
    0
}

pub fn fpathconf(mut _ctx: ContextMut<'_, EmEnv>, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::fpathconf");
    0
}

pub fn getitimer(mut _ctx: ContextMut<'_, EmEnv>, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::getitimer");
    0
}

pub fn getpwent(mut _ctx: ContextMut<'_, EmEnv>) -> i32 {
    debug!("emscripten::getpwent");
    0
}

pub fn killpg(mut _ctx: ContextMut<'_, EmEnv>, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::killpg");
    0
}

#[cfg(unix)]
pub fn pathconf(mut ctx: ContextMut<'_, EmEnv>, path_ptr: i32, name: i32) -> i32 {
    debug!("emscripten::pathconf");
    let path = emscripten_memory_pointer!(ctx, ctx.data().memory(0), path_ptr) as *const i8;
    unsafe { libc::pathconf(path as *const _, name).try_into().unwrap() }
}

#[cfg(not(unix))]
pub fn pathconf(mut _ctx: ContextMut<'_, EmEnv>, _path_ptr: i32, _name: i32) -> i32 {
    debug!("emscripten::pathconf");
    0
}

pub fn setpwent(mut _ctx: ContextMut<'_, EmEnv>) {
    debug!("emscripten::setpwent");
}

pub fn sigismember(mut _ctx: ContextMut<'_, EmEnv>, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::sigismember");
    0
}

pub fn sigpending(mut _ctx: ContextMut<'_, EmEnv>, _a: i32) -> i32 {
    debug!("emscripten::sigpending");
    0
}
