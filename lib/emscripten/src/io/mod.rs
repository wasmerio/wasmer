#[cfg(unix)]
mod unix;

#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use self::unix::*;

#[cfg(windows)]
pub use self::windows::*;

use crate::EmEnv;

/// getprotobyname
pub fn getprotobyname(_ctx: &EmEnv, _name_ptr: i32) -> i32 {
    debug!("emscripten::getprotobyname");
    unimplemented!("emscripten::getprotobyname")
}

/// getprotobynumber
pub fn getprotobynumber(_ctx: &EmEnv, _one: i32) -> i32 {
    debug!("emscripten::getprotobynumber");
    unimplemented!("emscripten::getprotobynumber")
}

/// sigdelset
pub fn sigdelset(ctx: &EmEnv, set: i32, signum: i32) -> i32 {
    debug!("emscripten::sigdelset");
    let memory = ctx.memory(0);
    #[allow(clippy::cast_ptr_alignment)]
    let ptr = emscripten_memory_pointer!(memory, set) as *mut i32;

    unsafe { *ptr &= !(1 << (signum - 1)) }

    0
}

/// sigfillset
pub fn sigfillset(ctx: &EmEnv, set: i32) -> i32 {
    debug!("emscripten::sigfillset");
    let memory = ctx.memory(0);
    #[allow(clippy::cast_ptr_alignment)]
    let ptr = emscripten_memory_pointer!(memory, set) as *mut i32;

    unsafe {
        *ptr = -1;
    }

    0
}

/// tzset
pub fn tzset(_ctx: &EmEnv) {
    debug!("emscripten::tzset - stub");
    //unimplemented!("emscripten::tzset - stub")
}

/// strptime
pub fn strptime(_ctx: &EmEnv, _one: i32, _two: i32, _three: i32) -> i32 {
    debug!("emscripten::strptime");
    unimplemented!("emscripten::strptime")
}
