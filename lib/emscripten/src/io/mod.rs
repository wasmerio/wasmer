#[cfg(unix)]
mod unix;

#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use self::unix::*;

#[cfg(windows)]
pub use self::windows::*;

use wasmer_runtime_core::vm::Ctx;

/// getprotobyname
pub fn getprotobyname(_ctx: &mut Ctx, _name_ptr: i32) -> i32 {
    debug!("emscripten::getprotobyname");
    unimplemented!()
}

/// getprotobynumber
pub fn getprotobynumber(_ctx: &mut Ctx, _one: i32) -> i32 {
    debug!("emscripten::getprotobynumber");
    unimplemented!()
}

/// sigdelset
pub fn sigdelset(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::sigdelset");
    unimplemented!()
}

/// sigfillset
pub fn sigfillset(_ctx: &mut Ctx, _one: i32) -> i32 {
    debug!("emscripten::sigfillset");
    unimplemented!()
}

/// tzset
pub fn tzset(_ctx: &mut Ctx) {
    debug!("emscripten::tzset");
    unimplemented!()
}

/// strptime
pub fn strptime(_ctx: &mut Ctx, _one: i32, _two: i32, _three: i32) -> i32 {
    debug!("emscripten::strptime");
    unimplemented!()
}
