#![allow(non_snake_case)]

use crate::env::get_emscripten_data;
#[cfg(target_os = "linux")]
use libc::getdtablesize;
use wasmer_runtime_core::vm::Ctx;

pub fn asm_const_i(_ctx: &mut Ctx, _val: i32) -> i32 {
    debug!("emscripten::asm_const_i: {}", _val);
    0
}

pub fn exit_with_live_runtime(_ctx: &mut Ctx) {
    debug!("emscripten::exit_with_live_runtime");
}

pub fn setTempRet0(ctx: &mut Ctx, val: i32) {
    debug!("emscripten::setTempRet0: {}", val);
    get_emscripten_data(ctx).temp_ret_0 = val;
}

pub fn getTempRet0(ctx: &mut Ctx) -> i32 {
    debug!("emscripten::getTempRet0");
    get_emscripten_data(ctx).temp_ret_0
}

pub fn _alarm(_ctx: &mut Ctx, _seconds: u32) -> i32 {
    debug!("emscripten::_alarm({})", _seconds);
    0
}

pub fn _atexit(_ctx: &mut Ctx, _func: i32) -> i32 {
    debug!("emscripten::_atexit");
    // TODO: implement atexit properly
    // __ATEXIT__.unshift({
    //     func: func,
    //     arg: arg
    // });
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
pub fn _pthread_attr_destroy(_ctx: &mut Ctx, _a: i32) -> i32 {
    debug!("emscripten::_pthread_attr_destroy");
    0
}
pub fn _pthread_attr_getstack(
    _ctx: &mut Ctx,
    _stackaddr: i32,
    _stacksize: i32,
    _other: i32,
) -> i32 {
    debug!(
        "emscripten::_pthread_attr_getstack({}, {}, {})",
        _stackaddr, _stacksize, _other
    );
    // TODO: Translate from Emscripten
    // HEAP32[stackaddr >> 2] = STACK_BASE;
    // HEAP32[stacksize >> 2] = TOTAL_STACK;
    0
}
pub fn _pthread_attr_init(_ctx: &mut Ctx, _a: i32) -> i32 {
    debug!("emscripten::_pthread_attr_init({})", _a);
    0
}
pub fn _pthread_attr_setstacksize(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::_pthread_attr_setstacksize");
    0
}
pub fn _pthread_cleanup_pop(_ctx: &mut Ctx, _a: i32) -> () {
    debug!("emscripten::_pthread_cleanup_pop");
}
pub fn _pthread_cleanup_push(_ctx: &mut Ctx, _a: i32, _b: i32) -> () {
    debug!("emscripten::_pthread_cleanup_push");
}
pub fn _pthread_cond_destroy(_ctx: &mut Ctx, _a: i32) -> i32 {
    debug!("emscripten::_pthread_cond_destroy");
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
pub fn _pthread_cond_timedwait(_ctx: &mut Ctx, _a: i32, _b: i32, _c: i32) -> i32 {
    debug!("emscripten::_pthread_cond_timedwait");
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
pub fn _pthread_create(_ctx: &mut Ctx, _a: i32, _b: i32, _c: i32, _d: i32) -> i32 {
    debug!("emscripten::_pthread_create");
    0
}
pub fn _pthread_detach(_ctx: &mut Ctx, _a: i32) -> i32 {
    debug!("emscripten::_pthread_detach");
    0
}
pub fn _pthread_equal(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::_pthread_equal");
    0
}
pub fn _pthread_exit(_ctx: &mut Ctx, _a: i32) -> () {
    debug!("emscripten::_pthread_exit");
}
pub fn _pthread_getattr_np(_ctx: &mut Ctx, _thread: i32, _attr: i32) -> i32 {
    debug!("emscripten::_pthread_getattr_np({}, {})", _thread, _attr);
    0
}
pub fn _pthread_getspecific(_ctx: &mut Ctx, _a: i32) -> i32 {
    debug!("emscripten::_pthread_getspecific");
    0
}
pub fn _pthread_join(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::_pthread_join");
    0
}
pub fn _pthread_key_create(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::_pthread_key_create");
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
pub fn _pthread_once(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::_pthread_once");
    0
}
pub fn _pthread_rwlock_destroy(_ctx: &mut Ctx, _rwlock: i32) -> i32 {
    debug!("emscripten::_pthread_rwlock_destroy({})", _rwlock);
    0
}
pub fn _pthread_rwlock_init(_ctx: &mut Ctx, _rwlock: i32, _attr: i32) -> i32 {
    debug!("emscripten::_pthread_rwlock_init({}, {})", _rwlock, _attr);
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
pub fn _pthread_rwlock_wrlock(_ctx: &mut Ctx, _rwlock: i32) -> i32 {
    debug!("emscripten::_pthread_rwlock_wrlock({})", _rwlock);
    0
}
pub fn _pthread_setcancelstate(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::_pthread_setcancelstate");
    0
}
pub fn _pthread_setspecific(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::_pthread_setspecific");
    0
}
pub fn _pthread_sigmask(_ctx: &mut Ctx, _a: i32, _b: i32, _c: i32) -> i32 {
    debug!("emscripten::_pthread_sigmask");
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

pub fn _gai_strerror(_ctx: &mut Ctx, _ecode: i32) -> i32 {
    debug!("emscripten::_gai_strerror({})", _ecode);
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
pub fn _gethostbyname(_ctx: &mut Ctx, _name: i32) -> i32 {
    debug!("emscripten::gethostbyname_r");
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
pub fn _getnameinfo(
    _ctx: &mut Ctx,
    _addr: i32,
    _addrlen: i32,
    _host: i32,
    _hostlen: i32,
    _serv: i32,
    _servlen: i32,
    _flags: i32,
) -> i32 {
    debug!(
        "emscripten::_getnameinfo({}, {}, {}, {}, {}, {}, {})",
        _addr, _addrlen, _host, _hostlen, _serv, _servlen, _flags
    );
    0
}

// Invoke functions
// They save the stack to allow unwinding

// Macro definitions
macro_rules! invoke {
    ($ctx: ident, $name:ident, $( $arg:ident ),*) => {{
        let sp = get_emscripten_data($ctx).stack_save.as_ref().expect("stack_save is None").call().expect("stack_save call failed");
        let result = get_emscripten_data($ctx).$name.as_ref().expect(concat!("Dynamic call is None: ", stringify!($name))).call($($arg),*);
        match result {
            Ok(v) => v,
            Err(_e) => {
                get_emscripten_data($ctx).stack_restore.as_ref().expect("stack_restore is None").call(sp).expect("stack_restore call failed");
                // TODO: We should check if _e != "longjmp" and if that's the case, re-throw the error
                // JS version is: if (e !== e+0 && e !== 'longjmp') throw e;
                get_emscripten_data($ctx).set_threw.as_ref().expect("set_threw is None").call(1, 0).expect("set_threw call failed");
                0 as _
            }
        }
    }};
}
macro_rules! invoke_no_return {
    ($ctx: ident, $name:ident, $( $arg:ident ),*) => {{
        let sp = get_emscripten_data($ctx).stack_save.as_ref().expect("stack_save is None").call().expect("stack_save call failed");
        let result = get_emscripten_data($ctx).$name.as_ref().expect(concat!("Dynamic call is None: ", stringify!($name))).call($($arg),*);
        match result {
            Ok(v) => v,
            Err(_e) => {
                get_emscripten_data($ctx).stack_restore.as_ref().expect("stack_restore is None").call(sp).expect("stack_restore call failed");
                // TODO: We should check if _e != "longjmp" and if that's the case, re-throw the error
                // JS version is: if (e !== e+0 && e !== 'longjmp') throw e;
                get_emscripten_data($ctx).set_threw.as_ref().expect("set_threw is None").call(1, 0).expect("set_threw call failed");
            }
        }
    }};
}

// Invoke functions
pub fn invoke_i(ctx: &mut Ctx, index: i32) -> i32 {
    debug!("emscripten::invoke_i");
    invoke!(ctx, dyn_call_i, index)
}
pub fn invoke_ii(ctx: &mut Ctx, index: i32, a1: i32) -> i32 {
    debug!("emscripten::invoke_ii");
    invoke!(ctx, dyn_call_ii, index, a1)
}
pub fn invoke_iii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32) -> i32 {
    debug!("emscripten::invoke_iii");
    invoke!(ctx, dyn_call_iii, index, a1, a2)
}
pub fn invoke_iiii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32) -> i32 {
    debug!("emscripten::invoke_iiii");
    invoke!(ctx, dyn_call_iiii, index, a1, a2, a3)
}
pub fn invoke_iifi(ctx: &mut Ctx, index: i32, a1: i32, a2: f64, a3: i32) -> i32 {
    debug!("emscripten::invoke_iifi");
    invoke!(ctx, dyn_call_iifi, index, a1, a2, a3)
}
pub fn invoke_v(ctx: &mut Ctx, index: i32) {
    debug!("emscripten::invoke_v");
    invoke_no_return!(ctx, dyn_call_v, index);
}
pub fn invoke_vi(ctx: &mut Ctx, index: i32, a1: i32) {
    debug!("emscripten::invoke_vi");
    invoke_no_return!(ctx, dyn_call_vi, index, a1);
}
pub fn invoke_vii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32) {
    debug!("emscripten::invoke_vii");
    invoke_no_return!(ctx, dyn_call_vii, index, a1, a2);
}

pub fn invoke_viii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32) {
    debug!("emscripten::invoke_viii");
    invoke_no_return!(ctx, dyn_call_viii, index, a1, a2, a3);
}
pub fn invoke_viiii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32, a4: i32) {
    debug!("emscripten::invoke_viiii");
    invoke_no_return!(ctx, dyn_call_viiii, index, a1, a2, a3, a4);
}
pub fn invoke_dii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32) -> f64 {
    debug!("emscripten::invoke_dii");
    invoke!(ctx, dyn_call_dii, index, a1, a2)
}
pub fn invoke_diiii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32, a4: i32) -> f64 {
    debug!("emscripten::invoke_diiii");
    invoke!(ctx, dyn_call_diiii, index, a1, a2, a3, a4)
}
pub fn invoke_iiiii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32, a4: i32) -> i32 {
    debug!("emscripten::invoke_iiiii");
    invoke!(ctx, dyn_call_iiiii, index, a1, a2, a3, a4)
}
pub fn invoke_iiiiii(
    ctx: &mut Ctx,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
) -> i32 {
    debug!("emscripten::invoke_iiiiii");
    invoke!(ctx, dyn_call_iiiiii, index, a1, a2, a3, a4, a5)
}
pub fn invoke_iiiiiii(
    ctx: &mut Ctx,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
    a6: i32,
) -> i32 {
    debug!("emscripten::invoke_iiiiiii");
    invoke!(ctx, dyn_call_iiiiiii, index, a1, a2, a3, a4, a5, a6)
}
pub fn invoke_iiiiiiii(
    ctx: &mut Ctx,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
    a6: i32,
    a7: i32,
) -> i32 {
    debug!("emscripten::invoke_iiiiiiii");
    invoke!(ctx, dyn_call_iiiiiiii, index, a1, a2, a3, a4, a5, a6, a7)
}
pub fn invoke_iiiiiiiii(
    ctx: &mut Ctx,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
    a6: i32,
    a7: i32,
    a8: i32,
) -> i32 {
    debug!("emscripten::invoke_iiiiiiiii");
    invoke!(
        ctx,
        dyn_call_iiiiiiiii,
        index,
        a1,
        a2,
        a3,
        a4,
        a5,
        a6,
        a7,
        a8
    )
}
pub fn invoke_iiiiiiiiii(
    ctx: &mut Ctx,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
    a6: i32,
    a7: i32,
    a8: i32,
    a9: i32,
) -> i32 {
    debug!("emscripten::invoke_iiiiiiiiii");
    invoke!(
        ctx,
        dyn_call_iiiiiiiiii,
        index,
        a1,
        a2,
        a3,
        a4,
        a5,
        a6,
        a7,
        a8,
        a9
    )
}
pub fn invoke_iiiiiiiiiii(
    ctx: &mut Ctx,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
    a6: i32,
    a7: i32,
    a8: i32,
    a9: i32,
    a10: i32,
) -> i32 {
    debug!("emscripten::invoke_iiiiiiiiiii");
    invoke!(
        ctx,
        dyn_call_iiiiiiiiiii,
        index,
        a1,
        a2,
        a3,
        a4,
        a5,
        a6,
        a7,
        a8,
        a9,
        a10
    )
}
pub fn invoke_vd(ctx: &mut Ctx, index: i32, a1: f64) {
    debug!("emscripten::invoke_vd");
    invoke_no_return!(ctx, dyn_call_vd, index, a1)
}
pub fn invoke_viiiii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32, a4: i32, a5: i32) {
    debug!("emscripten::invoke_viiiii");
    invoke_no_return!(ctx, dyn_call_viiiii, index, a1, a2, a3, a4, a5)
}
pub fn invoke_viiiiii(
    ctx: &mut Ctx,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
    a6: i32,
) {
    debug!("emscripten::invoke_viiiiii");
    invoke_no_return!(ctx, dyn_call_viiiiii, index, a1, a2, a3, a4, a5, a6)
}
pub fn invoke_viiiiiii(
    ctx: &mut Ctx,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
    a6: i32,
    a7: i32,
) {
    debug!("emscripten::invoke_viiiiiii");
    invoke_no_return!(ctx, dyn_call_viiiiiii, index, a1, a2, a3, a4, a5, a6, a7)
}
pub fn invoke_viiiiiiii(
    ctx: &mut Ctx,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
    a6: i32,
    a7: i32,
    a8: i32,
) {
    debug!("emscripten::invoke_viiiiiiii");
    invoke_no_return!(
        ctx,
        dyn_call_viiiiiiii,
        index,
        a1,
        a2,
        a3,
        a4,
        a5,
        a6,
        a7,
        a8
    )
}
pub fn invoke_viiiiiiiii(
    ctx: &mut Ctx,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
    a6: i32,
    a7: i32,
    a8: i32,
    a9: i32,
) {
    debug!("emscripten::invoke_viiiiiiiii");
    invoke_no_return!(
        ctx,
        dyn_call_viiiiiiiii,
        index,
        a1,
        a2,
        a3,
        a4,
        a5,
        a6,
        a7,
        a8,
        a9
    )
}
pub fn invoke_viiiiiiiiii(
    ctx: &mut Ctx,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
    a6: i32,
    a7: i32,
    a8: i32,
    a9: i32,
    a10: i32,
) {
    debug!("emscripten::invoke_viiiiiiiiii");
    invoke_no_return!(
        ctx,
        dyn_call_viiiiiiiiii,
        index,
        a1,
        a2,
        a3,
        a4,
        a5,
        a6,
        a7,
        a8,
        a9,
        a10
    )
}

pub fn invoke_iij(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32) -> i32 {
    debug!("emscripten::invoke_iij");
    invoke!(ctx, dyn_call_iij, index, a1, a2, a3)
}

pub fn invoke_iji(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32) -> i32 {
    debug!("emscripten::invoke_iji");
    invoke!(ctx, dyn_call_iji, index, a1, a2, a3)
}

pub fn invoke_iiji(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32, a4: i32) -> i32 {
    debug!("emscripten::invoke_iiji");
    invoke!(ctx, dyn_call_iiji, index, a1, a2, a3, a4)
}

pub fn invoke_iiijj(
    ctx: &mut Ctx,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
    a6: i32,
) -> i32 {
    debug!("emscripten::invoke_iiijj");
    invoke!(ctx, dyn_call_iiijj, index, a1, a2, a3, a4, a5, a6)
}
pub fn invoke_j(ctx: &mut Ctx, index: i32) -> i32 {
    debug!("emscripten::invoke_j");
    if let Some(dyn_call_j) = &get_emscripten_data(ctx).dyn_call_j {
        dyn_call_j.call(index).unwrap()
    } else {
        panic!("dyn_call_j is set to None");
    }
}
pub fn invoke_ji(ctx: &mut Ctx, index: i32, a1: i32) -> i32 {
    debug!("emscripten::invoke_ji");
    if let Some(dyn_call_ji) = &get_emscripten_data(ctx).dyn_call_ji {
        dyn_call_ji.call(index, a1).unwrap()
    } else {
        panic!("dyn_call_ji is set to None");
    }
}
pub fn invoke_jii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32) -> i32 {
    debug!("emscripten::invoke_jii");
    if let Some(dyn_call_jii) = &get_emscripten_data(ctx).dyn_call_jii {
        dyn_call_jii.call(index, a1, a2).unwrap()
    } else {
        panic!("dyn_call_jii is set to None");
    }
}

pub fn invoke_jij(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32) -> i32 {
    debug!("emscripten::invoke_jij");
    if let Some(dyn_call_jij) = &get_emscripten_data(ctx).dyn_call_jij {
        dyn_call_jij.call(index, a1, a2, a3).unwrap()
    } else {
        panic!("dyn_call_jij is set to None");
    }
}
pub fn invoke_jjj(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32, a4: i32) -> i32 {
    debug!("emscripten::invoke_jjj");
    if let Some(dyn_call_jjj) = &get_emscripten_data(ctx).dyn_call_jjj {
        dyn_call_jjj.call(index, a1, a2, a3, a4).unwrap()
    } else {
        panic!("dyn_call_jjj is set to None");
    }
}
pub fn invoke_viiij(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32, a4: i32, a5: i32) {
    debug!("emscripten::invoke_viiij");
    if let Some(dyn_call_viiij) = &get_emscripten_data(ctx).dyn_call_viiij {
        dyn_call_viiij.call(index, a1, a2, a3, a4, a5).unwrap();
    } else {
        panic!("dyn_call_viiij is set to None");
    }
}
pub fn invoke_viiijiiii(
    ctx: &mut Ctx,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
    a6: i32,
    a7: i32,
    a8: i32,
    a9: i32,
) {
    debug!("emscripten::invoke_viiijiiii");
    if let Some(dyn_call_viiijiiii) = &get_emscripten_data(ctx).dyn_call_viiijiiii {
        dyn_call_viiijiiii
            .call(index, a1, a2, a3, a4, a5, a6, a7, a8, a9)
            .unwrap();
    } else {
        panic!("dyn_call_viiijiiii is set to None");
    }
}
pub fn invoke_viiijiiiiii(
    ctx: &mut Ctx,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
    a6: i32,
    a7: i32,
    a8: i32,
    a9: i32,
    a10: i32,
    a11: i32,
) {
    debug!("emscripten::invoke_viiijiiiiii");
    if let Some(dyn_call_viiijiiiiii) = &get_emscripten_data(ctx).dyn_call_viiijiiiiii {
        dyn_call_viiijiiiiii
            .call(index, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11)
            .unwrap();
    } else {
        panic!("dyn_call_viiijiiiiii is set to None");
    }
}
pub fn invoke_viij(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32, a4: i32) {
    debug!("emscripten::invoke_viij");
    if let Some(dyn_call_viij) = &get_emscripten_data(ctx).dyn_call_viij {
        dyn_call_viij.call(index, a1, a2, a3, a4).unwrap();
    } else {
        panic!("dyn_call_viij is set to None");
    }
}
pub fn invoke_viiji(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32, a4: i32, a5: i32) {
    debug!("emscripten::invoke_viiji");
    if let Some(dyn_call_viiji) = &get_emscripten_data(ctx).dyn_call_viiji {
        dyn_call_viiji.call(index, a1, a2, a3, a4, a5).unwrap();
    } else {
        panic!("dyn_call_viiji is set to None");
    }
}
pub fn invoke_viijiii(
    ctx: &mut Ctx,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
    a6: i32,
    a7: i32,
) {
    debug!("emscripten::invoke_viijiii");
    if let Some(dyn_call_viijiii) = &get_emscripten_data(ctx).dyn_call_viijiii {
        dyn_call_viijiii
            .call(index, a1, a2, a3, a4, a5, a6, a7)
            .unwrap();
    } else {
        panic!("dyn_call_viijiii is set to None");
    }
}
pub fn invoke_viijj(
    ctx: &mut Ctx,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
    a6: i32,
) {
    debug!("emscripten::invoke_viijj");
    if let Some(dyn_call_viijj) = &get_emscripten_data(ctx).dyn_call_viijj {
        dyn_call_viijj.call(index, a1, a2, a3, a4, a5, a6).unwrap();
    } else {
        panic!("dyn_call_viijj is set to None");
    }
}
pub fn invoke_vj(ctx: &mut Ctx, index: i32, a1: i32, a2: i32) {
    debug!("emscripten::invoke_vj");
    if let Some(dyn_call_vj) = &get_emscripten_data(ctx).dyn_call_vj {
        dyn_call_vj.call(index, a1, a2).unwrap();
    } else {
        panic!("dyn_call_vj is set to None");
    }
}
pub fn invoke_vjji(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32, a4: i32, a5: i32) {
    debug!("emscripten::invoke_vjji");
    invoke_no_return!(ctx, dyn_call_vjji, index, a1, a2, a3, a4, a5)
}
pub fn invoke_vij(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32) {
    debug!("emscripten::invoke_vij");
    if let Some(dyn_call_vij) = &get_emscripten_data(ctx).dyn_call_vij {
        dyn_call_vij.call(index, a1, a2, a3).unwrap();
    } else {
        panic!("dyn_call_vij is set to None");
    }
}
pub fn invoke_viji(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32, a4: i32) {
    debug!("emscripten::invoke_viji");
    if let Some(dyn_call_viji) = &get_emscripten_data(ctx).dyn_call_viji {
        dyn_call_viji.call(index, a1, a2, a3, a4).unwrap()
    } else {
        panic!("dyn_call_viji is set to None");
    }
}
pub fn invoke_vijiii(
    ctx: &mut Ctx,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
    a6: i32,
) {
    debug!("emscripten::invoke_vijiii");
    if let Some(dyn_call_vijiii) = &get_emscripten_data(ctx).dyn_call_vijiii {
        dyn_call_vijiii.call(index, a1, a2, a3, a4, a5, a6).unwrap()
    } else {
        panic!("dyn_call_vijiii is set to None");
    }
}
pub fn invoke_vijj(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32, a4: i32, a5: i32) {
    debug!("emscripten::invoke_vijj");
    if let Some(dyn_call_vijj) = &get_emscripten_data(ctx).dyn_call_vijj {
        dyn_call_vijj.call(index, a1, a2, a3, a4, a5).unwrap()
    } else {
        panic!("dyn_call_vijj is set to None");
    }
}
pub fn invoke_vidd(ctx: &mut Ctx, index: i32, a1: i32, a2: f64, a3: f64) {
    debug!("emscripten::invoke_viid");
    invoke_no_return!(ctx, dyn_call_vidd, index, a1, a2, a3);
}
pub fn invoke_viid(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: f64) {
    debug!("emscripten::invoke_viid");
    invoke_no_return!(ctx, dyn_call_viid, index, a1, a2, a3);
}
pub fn invoke_viidii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: f64, a4: i32, a5: i32) {
    debug!("emscripten::invoke_viidii");
    invoke_no_return!(ctx, dyn_call_viidii, index, a1, a2, a3, a4, a5);
}
pub fn invoke_viidddddddd(
    ctx: &mut Ctx,
    index: i32,
    a1: i32,
    a2: i32,
    a3: f64,
    a4: f64,
    a5: f64,
    a6: f64,
    a7: f64,
    a8: f64,
    a9: f64,
    a10: f64,
) {
    debug!("emscripten::invoke_viidddddddd");
    invoke_no_return!(
        ctx,
        dyn_call_viidddddddd,
        index,
        a1,
        a2,
        a3,
        a4,
        a5,
        a6,
        a7,
        a8,
        a9,
        a10
    );
}
