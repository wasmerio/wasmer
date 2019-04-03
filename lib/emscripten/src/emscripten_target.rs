#![allow(non_snake_case)]

#[cfg(target_os = "linux")]
use libc::getdtablesize;
use wasmer_runtime_core::vm::Ctx;

pub fn setTempRet0(_ctx: &mut Ctx, _a: i32) {
    debug!("emscripten::setTempRet0");
}
pub fn getTempRet0(_ctx: &mut Ctx) -> i32 {
    debug!("emscripten::getTempRet0");
    0
}
pub fn __Unwind_Backtrace(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::__Unwind_Backtrace");
    0
}
pub fn __Unwind_FindEnclosingFunction(_ctx: &mut Ctx, _a: i32) -> i32 {
    debug!("emscripten::__Unwind_FindEnclosingFunction");
    0
}
pub fn __Unwind_GetIPInfo(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::__Unwind_GetIPInfo");
    0
}
pub fn ___cxa_find_matching_catch_2(_ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___cxa_find_matching_catch_2");
    0
}
pub fn ___cxa_find_matching_catch_3(_ctx: &mut Ctx, _a: i32) -> i32 {
    debug!("emscripten::___cxa_find_matching_catch_3");
    0
}
pub fn ___cxa_free_exception(_ctx: &mut Ctx, _a: i32) {
    debug!("emscripten::___cxa_free_exception");
}
pub fn ___resumeException(_ctx: &mut Ctx, _a: i32) {
    debug!("emscripten::___resumeException");
}
pub fn _dladdr(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::_dladdr");
    0
}
pub fn _pthread_cond_destroy(_ctx: &mut Ctx, _a: i32) -> i32 {
    debug!("emscripten::_pthread_cond_destroy");
    0
}
pub fn _pthread_getspecific(_ctx: &mut Ctx, _a: i32) -> i32 {
    debug!("emscripten::_pthread_getspecific");
    0
}
pub fn _pthread_setspecific(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::_pthread_setspecific");
    0
}
pub fn _pthread_once(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::_pthread_once");
    0
}
pub fn _pthread_key_create(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::_pthread_key_create");
    0
}
pub fn _pthread_create(_ctx: &mut Ctx, _a: i32, _b: i32, _c: i32, _d: i32) -> i32 {
    debug!("emscripten::_pthread_create");
    0
}
pub fn _pthread_join(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::_pthread_join");
    0
}
pub fn _pthread_cond_init(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::_pthread_cond_init");
    0
}
pub fn _pthread_cond_signal(_ctx: &mut Ctx, _a: i32) -> i32 {
    debug!("emscripten::_pthread_cond_signal");
    0
}
pub fn _pthread_cond_wait(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::_pthread_cond_wait");
    0
}
pub fn _pthread_condattr_destroy(_ctx: &mut Ctx, _a: i32) -> i32 {
    debug!("emscripten::_pthread_condattr_destroy");
    0
}
pub fn _pthread_condattr_init(_ctx: &mut Ctx, _a: i32) -> i32 {
    debug!("emscripten::_pthread_condattr_init");
    0
}
pub fn _pthread_condattr_setclock(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::_pthread_condattr_setclock");
    0
}
pub fn _pthread_mutex_destroy(_ctx: &mut Ctx, _a: i32) -> i32 {
    debug!("emscripten::_pthread_mutex_destroy");
    0
}
pub fn _pthread_mutex_init(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::_pthread_mutex_init");
    0
}
pub fn _pthread_mutexattr_destroy(_ctx: &mut Ctx, _a: i32) -> i32 {
    debug!("emscripten::_pthread_mutexattr_destroy");
    0
}
pub fn _pthread_mutexattr_init(_ctx: &mut Ctx, _a: i32) -> i32 {
    debug!("emscripten::_pthread_mutexattr_init");
    0
}
pub fn _pthread_mutexattr_settype(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::_pthread_mutexattr_settype");
    0
}
pub fn _pthread_rwlock_rdlock(_ctx: &mut Ctx, _a: i32) -> i32 {
    debug!("emscripten::_pthread_rwlock_rdlock");
    0
}
pub fn _pthread_rwlock_unlock(_ctx: &mut Ctx, _a: i32) -> i32 {
    debug!("emscripten::_pthread_rwlock_unlock");
    0
}
pub fn _pthread_setcancelstate(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::_pthread_setcancelstate");
    0
}
pub fn ___gxx_personality_v0(
    _ctx: &mut Ctx,
    _a: i32,
    _b: i32,
    _c: i32,
    _d: i32,
    _e: i32,
    _f: i32,
) -> i32 {
    debug!("emscripten::___gxx_personality_v0");
    0
}
#[cfg(target_os = "linux")]
pub fn _getdtablesize(_ctx: &mut Ctx) -> i32 {
    debug!("emscripten::getdtablesize");
    unsafe { getdtablesize() }
}
#[cfg(not(target_os = "linux"))]
pub fn _getdtablesize(_ctx: &mut Ctx) -> i32 {
    debug!("emscripten::getdtablesize");
    -1
}
pub fn _gethostbyaddr(_ctx: &mut Ctx, _addr: i32, _addrlen: i32, _atype: i32) -> i32 {
    debug!("emscripten::gethostbyaddr");
    0
}
pub fn _gethostbyname_r(
    _ctx: &mut Ctx,
    _name: i32,
    _ret: i32,
    _buf: i32,
    _buflen: i32,
    _out: i32,
    _err: i32,
) -> i32 {
    debug!("emscripten::gethostbyname_r");
    0
}
// NOTE: php.js has proper impl; libc has proper impl for linux
pub fn _getloadavg(_ctx: &mut Ctx, _loadavg: i32, _nelem: i32) -> i32 {
    debug!("emscripten::getloadavg");
    0
}
