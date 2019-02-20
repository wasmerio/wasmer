use crate::env::get_emscripten_data;
use wasmer_runtime_core::vm::Ctx;

pub fn setTempRet0(ctx: &mut Ctx, a: i32) {
    debug!("emscripten::setTempRet0");
}
pub fn getTempRet0(ctx: &mut Ctx) -> i32 {
    debug!("emscripten::getTempRet0");
    0
}
pub fn nullFunc_ji(ctx: &mut Ctx, a: i32) {
    debug!("emscripten::nullFunc_ji");
}
pub fn invoke_i(ctx: &mut Ctx, index: i32) -> i32 {
    debug!("emscripten::invoke_i");
    get_emscripten_data(ctx).dyn_call_i.call(index).unwrap()
}
pub fn invoke_ii(ctx: &mut Ctx, index: i32, a1: i32) -> i32 {
    debug!("emscripten::invoke_ii");
    get_emscripten_data(ctx)
        .dyn_call_ii
        .call(index, a1)
        .unwrap()
}
pub fn invoke_iii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32) -> i32 {
    debug!("emscripten::invoke_iii");
    get_emscripten_data(ctx)
        .dyn_call_iii
        .call(index, a1, a2)
        .unwrap()
}
pub fn invoke_iiii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32) -> i32 {
    debug!("emscripten::invoke_iiii");
    get_emscripten_data(ctx)
        .dyn_call_iiii
        .call(index, a1, a2, a3)
        .unwrap()
}
pub fn invoke_v(ctx: &mut Ctx, index: i32) {
    debug!("emscripten::invoke_v");
    get_emscripten_data(ctx).dyn_call_v.call(index).unwrap();
}
pub fn invoke_vi(ctx: &mut Ctx, index: i32, a1: i32) {
    debug!("emscripten::invoke_vi");
    get_emscripten_data(ctx)
        .dyn_call_vi
        .call(index, a1)
        .unwrap();
}
pub fn invoke_vii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32) {
    debug!("emscripten::invoke_vii");
    get_emscripten_data(ctx)
        .dyn_call_vii
        .call(index, a1, a2)
        .unwrap();
}
pub fn invoke_viii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32) {
    debug!("emscripten::invoke_viii");
    get_emscripten_data(ctx)
        .dyn_call_viii
        .call(index, a1, a2, a3)
        .unwrap();
}
pub fn invoke_viiii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32, a4: i32) {
    debug!("emscripten::invoke_viiii");
    get_emscripten_data(ctx)
        .dyn_call_viiii
        .call(index, a1, a2, a3, a4)
        .unwrap();
}
pub fn __Unwind_Backtrace(ctx: &mut Ctx, a: i32, b: i32) -> i32 {
    debug!("emscripten::__Unwind_Backtrace");
    0
}
pub fn __Unwind_FindEnclosingFunction(ctx: &mut Ctx, a: i32) -> i32 {
    debug!("emscripten::__Unwind_FindEnclosingFunction");
    0
}
pub fn __Unwind_GetIPInfo(ctx: &mut Ctx, a: i32, b: i32) -> i32 {
    debug!("emscripten::__Unwind_GetIPInfo");
    0
}
pub fn ___cxa_find_matching_catch_2(ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___cxa_find_matching_catch_2");
    0
}
pub fn ___cxa_find_matching_catch_3(ctx: &mut Ctx, a: i32) -> i32 {
    debug!("emscripten::___cxa_find_matching_catch_3");
    0
}
pub fn ___cxa_free_exception(ctx: &mut Ctx, a: i32) {
    debug!("emscripten::___cxa_free_exception");
}
pub fn ___resumeException(ctx: &mut Ctx, a: i32) {
    debug!("emscripten::___resumeException");
}
pub fn _dladdr(ctx: &mut Ctx, a: i32, b: i32) -> i32 {
    debug!("emscripten::_dladdr");
    0
}
pub fn _pthread_cond_destroy(ctx: &mut Ctx, a: i32) -> i32 {
    debug!("emscripten::_pthread_cond_destroy");
    0
}
pub fn _pthread_cond_init(ctx: &mut Ctx, a: i32, b: i32) -> i32 {
    debug!("emscripten::_pthread_cond_init");
    0
}
pub fn _pthread_cond_signal(ctx: &mut Ctx, a: i32) -> i32 {
    debug!("emscripten::_pthread_cond_signal");
    0
}
pub fn _pthread_cond_wait(ctx: &mut Ctx, a: i32, b: i32) -> i32 {
    debug!("emscripten::_pthread_cond_wait");
    0
}
pub fn _pthread_condattr_destroy(ctx: &mut Ctx, a: i32) -> i32 {
    debug!("emscripten::_pthread_condattr_destroy");
    0
}
pub fn _pthread_condattr_init(ctx: &mut Ctx, a: i32) -> i32 {
    debug!("emscripten::_pthread_condattr_init");
    0
}
pub fn _pthread_condattr_setclock(ctx: &mut Ctx, a: i32, b: i32) -> i32 {
    debug!("emscripten::_pthread_condattr_setclock");
    0
}
pub fn _pthread_mutex_destroy(ctx: &mut Ctx, a: i32) -> i32 {
    debug!("emscripten::_pthread_mutex_destroy");
    0
}
pub fn _pthread_mutex_init(ctx: &mut Ctx, a: i32, b: i32) -> i32 {
    debug!("emscripten::_pthread_mutex_init");
    0
}
pub fn _pthread_mutexattr_destroy(ctx: &mut Ctx, a: i32) -> i32 {
    debug!("emscripten::_pthread_mutexattr_destroy");
    0
}
pub fn _pthread_mutexattr_init(ctx: &mut Ctx, a: i32) -> i32 {
    debug!("emscripten::_pthread_mutexattr_init");
    0
}
pub fn _pthread_mutexattr_settype(ctx: &mut Ctx, a: i32, b: i32) -> i32 {
    debug!("emscripten::_pthread_mutexattr_settype");
    0
}
pub fn _pthread_rwlock_rdlock(ctx: &mut Ctx, a: i32) -> i32 {
    debug!("emscripten::_pthread_rwlock_rdlock");
    0
}
pub fn _pthread_rwlock_unlock(ctx: &mut Ctx, a: i32) -> i32 {
    debug!("emscripten::_pthread_rwlock_unlock");
    0
}
pub fn ___gxx_personality_v0(ctx: &mut Ctx, a: i32, b: i32, c: i32, d: i32, e: i32, f: i32) -> i32 {
    debug!("emscripten::___gxx_personality_v0");
    0
}
