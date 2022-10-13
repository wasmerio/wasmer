#[cfg(unix)]
mod unix;

#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use self::unix::*;

#[cfg(windows)]
pub use self::windows::*;

use crate::EmEnv;
use wasmer::FunctionEnvMut;

/// getprotobyname
pub fn getprotobyname(_ctx: FunctionEnvMut<EmEnv>, _name_ptr: i32) -> i32 {
    debug!("emscripten::getprotobyname");
    unimplemented!("emscripten::getprotobyname")
}

/// getprotobynumber
pub fn getprotobynumber(_ctx: FunctionEnvMut<EmEnv>, _one: i32) -> i32 {
    debug!("emscripten::getprotobynumber");
    unimplemented!("emscripten::getprotobynumber")
}

/// sigdelset
pub fn sigdelset(ctx: FunctionEnvMut<EmEnv>, set: i32, signum: i32) -> i32 {
    debug!("emscripten::sigdelset");
    let memory = ctx.data().memory(0);
    let view = memory.view(&ctx);
    #[allow(clippy::cast_ptr_alignment)]
    let ptr = emscripten_memory_pointer!(&view, set) as *mut i32;

    unsafe { *ptr &= !(1 << (signum - 1)) }

    0
}

/// sigfillset
pub fn sigfillset(ctx: FunctionEnvMut<EmEnv>, set: i32) -> i32 {
    debug!("emscripten::sigfillset");
    let memory = ctx.data().memory(0);
    let view = memory.view(&ctx);
    #[allow(clippy::cast_ptr_alignment)]
    let ptr = emscripten_memory_pointer!(&view, set) as *mut i32;

    unsafe {
        *ptr = -1;
    }

    0
}

/// tzset
pub fn tzset(_ctx: FunctionEnvMut<EmEnv>) {
    debug!("emscripten::tzset - stub");
    //unimplemented!("emscripten::tzset - stub")
}

/// strptime
pub fn strptime(_ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32, _three: i32) -> i32 {
    debug!("emscripten::strptime");
    unimplemented!("emscripten::strptime")
}
