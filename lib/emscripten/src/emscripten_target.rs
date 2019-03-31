#![allow(non_snake_case)]

use crate::env::get_emscripten_data;
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
pub fn invoke_i(ctx: &mut Ctx, index: i32) -> i32 {
    debug!("emscripten::invoke_i");
    if let Some(dyn_call_i) = &get_emscripten_data(ctx).dyn_call_i {
        dyn_call_i.call(index).unwrap()
    } else {
        panic!("dyn_call_i is set to None");
    }
}
pub fn invoke_ii(ctx: &mut Ctx, index: i32, a1: i32) -> i32 {
    debug!("emscripten::invoke_ii");
    if let Some(dyn_call_ii) = &get_emscripten_data(ctx).dyn_call_ii {
        dyn_call_ii.call(index, a1).unwrap()
    } else {
        panic!("dyn_call_ii is set to None");
    }
}
pub fn invoke_iii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32) -> i32 {
    debug!("emscripten::invoke_iii");
    if let Some(dyn_call_iii) = &get_emscripten_data(ctx).dyn_call_iii {
        dyn_call_iii.call(index, a1, a2).unwrap()
    } else {
        panic!("dyn_call_iii is set to None");
    }
}
pub fn invoke_iiii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32) -> i32 {
    debug!("emscripten::invoke_iiii");
    if let Some(dyn_call_iiii) = &get_emscripten_data(ctx).dyn_call_iiii {
        dyn_call_iiii.call(index, a1, a2, a3).unwrap()
    } else {
        panic!("dyn_call_iiii is set to None");
    }
}
pub fn invoke_v(ctx: &mut Ctx, index: i32) {
    debug!("emscripten::invoke_v");
    if let Some(dyn_call_v) = &get_emscripten_data(ctx).dyn_call_v {
        dyn_call_v.call(index).unwrap();
    } else {
        panic!("dyn_call_v is set to None");
    }
}
pub fn invoke_vi(ctx: &mut Ctx, index: i32, a1: i32) {
    debug!("emscripten::invoke_vi");
    if let Some(dyn_call_vi) = &get_emscripten_data(ctx).dyn_call_vi {
        dyn_call_vi.call(index, a1).unwrap();
    } else {
        panic!("dyn_call_vi is set to None");
    }
}
pub fn invoke_vii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32) {
    debug!("emscripten::invoke_vii");
    if let Some(dyn_call_vii) = &get_emscripten_data(ctx).dyn_call_vii {
        dyn_call_vii.call(index, a1, a2).unwrap();
    } else {
        panic!("dyn_call_vii is set to None");
    }
}
pub fn invoke_viii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32) {
    debug!("emscripten::invoke_viii");
    if let Some(dyn_call_viii) = &get_emscripten_data(ctx).dyn_call_viii {
        dyn_call_viii.call(index, a1, a2, a3).unwrap();
    } else {
        panic!("dyn_call_viii is set to None");
    }
}
pub fn invoke_viiii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32, a4: i32) {
    debug!("emscripten::invoke_viiii");
    if let Some(dyn_call_viiii) = &get_emscripten_data(ctx).dyn_call_viiii {
        dyn_call_viiii.call(index, a1, a2, a3, a4).unwrap();
    } else {
        panic!("dyn_call_viiii is set to None");
    }
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
pub fn invoke_dii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32) -> f64 {
    debug!("emscripten::invoke_dii");
    if let Some(dyn_call_dii) = &get_emscripten_data(ctx).dyn_call_dii {
        dyn_call_dii.call(index, a1, a2).unwrap()
    } else {
        panic!("dyn_call_dii is set to None");
    }
}
pub fn invoke_diiii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32, a4: i32) -> f64 {
    debug!("emscripten::invoke_diiii");
    if let Some(dyn_call_diiii) = &get_emscripten_data(ctx).dyn_call_diiii {
        dyn_call_diiii.call(index, a1, a2, a3, a4).unwrap()
    } else {
        panic!("dyn_call_diiii is set to None");
    }
}
pub fn invoke_iiiii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32, a4: i32) -> i32 {
    debug!("emscripten::invoke_iiiii");
    if let Some(dyn_call_iiiii) = &get_emscripten_data(ctx).dyn_call_iiiii {
        dyn_call_iiiii.call(index, a1, a2, a3, a4).unwrap()
    } else {
        panic!("dyn_call_iiiii is set to None");
    }
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
    if let Some(dyn_call_iiiiii) = &get_emscripten_data(ctx).dyn_call_iiiiii {
        dyn_call_iiiiii.call(index, a1, a2, a3, a4, a5).unwrap()
    } else {
        panic!("dyn_call_iiiiii is set to None");
    }
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
    if let Some(dyn_call_iiiiiii) = &get_emscripten_data(ctx).dyn_call_iiiiiii {
        dyn_call_iiiiiii
            .call(index, a1, a2, a3, a4, a5, a6)
            .unwrap()
    } else {
        panic!("dyn_call_iiiiiii is set to None");
    }
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
    if let Some(dyn_call_iiiiiiii) = &get_emscripten_data(ctx).dyn_call_iiiiiiii {
        dyn_call_iiiiiiii
            .call(index, a1, a2, a3, a4, a5, a6, a7)
            .unwrap()
    } else {
        panic!("dyn_call_iiiiiiii is set to None");
    }
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
    if let Some(dyn_call_iiiiiiiiii) = &get_emscripten_data(ctx).dyn_call_iiiiiiiiii {
        dyn_call_iiiiiiiiii
            .call(index, a1, a2, a3, a4, a5, a6, a7, a8, a9)
            .unwrap()
    } else {
        panic!("dyn_call_iiiiiiiiii is set to None");
    }
}
pub fn invoke_vd(ctx: &mut Ctx, index: i32, a1: f64) {
    debug!("emscripten::invoke_vd");
    if let Some(dyn_call_vd) = &get_emscripten_data(ctx).dyn_call_vd {
        dyn_call_vd.call(index, a1).unwrap();
    } else {
        panic!("dyn_call_vd is set to None");
    }
}
pub fn invoke_viiiii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32, a4: i32, a5: i32) {
    debug!("emscripten::invoke_viiiii");
    if let Some(dyn_call_viiiii) = &get_emscripten_data(ctx).dyn_call_viiiii {
        dyn_call_viiiii.call(index, a1, a2, a3, a4, a5).unwrap();
    } else {
        panic!("dyn_call_viiiii is set to None");
    }
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
    if let Some(dyn_call_viiiiii) = &get_emscripten_data(ctx).dyn_call_viiiiii {
        dyn_call_viiiiii
            .call(index, a1, a2, a3, a4, a5, a6)
            .unwrap();
    } else {
        panic!("dyn_call_viiiiii is set to None");
    }
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
    if let Some(dyn_call_viiiiiii) = &get_emscripten_data(ctx).dyn_call_viiiiiii {
        dyn_call_viiiiiii
            .call(index, a1, a2, a3, a4, a5, a6, a7)
            .unwrap();
    } else {
        panic!("dyn_call_viiiiiii is set to None");
    }
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
    if let Some(dyn_call_viiiiiiii) = &get_emscripten_data(ctx).dyn_call_viiiiiiii {
        dyn_call_viiiiiiii
            .call(index, a1, a2, a3, a4, a5, a6, a7, a8)
            .unwrap();
    } else {
        panic!("dyn_call_viiiiiiii is set to None");
    }
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
    if let Some(dyn_call_viiiiiiiii) = &get_emscripten_data(ctx).dyn_call_viiiiiiiii {
        dyn_call_viiiiiiiii
            .call(index, a1, a2, a3, a4, a5, a6, a7, a8, a9)
            .unwrap();
    } else {
        panic!("dyn_call_viiiiiiiii is set to None");
    }
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
    if let Some(dyn_call_viiiiiiiiii) = &get_emscripten_data(ctx).dyn_call_viiiiiiiiii {
        dyn_call_viiiiiiiiii
            .call(index, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10)
            .unwrap();
    } else {
        panic!("dyn_call_viiiiiiiiii is set to None");
    }
}

pub fn invoke_iiji(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: i32, a4: i32) -> i32 {
    debug!("emscripten::invoke_iiji");
    if let Some(dyn_call_iiji) = &get_emscripten_data(ctx).dyn_call_iiji {
        dyn_call_iiji.call(index, a1, a2, a3, a4).unwrap()
    } else {
        panic!("dyn_call_iiji is set to None");
    }
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
pub fn invoke_viidii(ctx: &mut Ctx, index: i32, a1: i32, a2: i32, a3: f64, a4: i32, a5: i32) {
    debug!("emscripten::invoke_viidii");
    if let Some(dyn_call_viidii) = &get_emscripten_data(ctx).dyn_call_viidii {
        dyn_call_viidii.call(index, a1, a2, a3, a4, a5).unwrap();
    } else {
        panic!("dyn_call_viidii is set to None");
    }
}
