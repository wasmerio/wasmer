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
// round 2
pub fn nullFunc_dii(ctx: &mut Ctx) {
    debug!("emscripten::nullFunc_dii");
}
pub fn nullFunc_diiii(ctx: &mut Ctx) {
    debug!("emscripten::nullFunc_diiii");
}
pub fn nullFunc_iiji(ctx: &mut Ctx) {
    debug!("emscripten::nullFunc_iiji");
}
pub fn nullFunc_j(ctx: &mut Ctx) {
    debug!("emscripten::nullFunc_j");
}
pub fn nullFunc_jij(ctx: &mut Ctx) {
    debug!("emscripten::nullFunc_jij");
}
pub fn nullFunc_jjj(ctx: &mut Ctx) {
    debug!("emscripten::nullFunc_jjj");
}
pub fn nullFunc_vd(ctx: &mut Ctx) {
    debug!("emscripten::nullFunc_vd");
}
pub fn nullFunc_viiiiiii(ctx: &mut Ctx) {
    debug!("emscripten::nullFunc_viiiiiii");
}
pub fn nullFunc_viiiiiiii(ctx: &mut Ctx) {
    debug!("emscripten::nullFunc_viiiiiiii");
}
pub fn nullFunc_viiiiiiiii(ctx: &mut Ctx) {
    debug!("emscripten::nullFunc_viiiiiiiii");
}
pub fn nullFunc_viiij(ctx: &mut Ctx) {
    debug!("emscripten::nullFunc_viiij");
}
pub fn nullFunc_viiijiiii(ctx: &mut Ctx) {
    debug!("emscripten::nullFunc_viiijiiii");
}
pub fn nullFunc_viiijiiiiii(ctx: &mut Ctx) {
    debug!("emscripten::nullFunc_viiijiiiiii");
}
pub fn nullFunc_viij(ctx: &mut Ctx) {
    debug!("emscripten::nullFunc_viij");
}
pub fn nullFunc_viiji(ctx: &mut Ctx) {
    debug!("emscripten::nullFunc_viiji");
}
pub fn nullFunc_viijiii(ctx: &mut Ctx) {
    debug!("emscripten::nullFunc_viijiii");
}
pub fn nullFunc_viijj(ctx: &mut Ctx) {
    debug!("emscripten::nullFunc_viijj");
}
pub fn nullFunc_vij(ctx: &mut Ctx) {
    debug!("emscripten::nullFunc_vij");
}
pub fn nullFunc_viji(ctx: &mut Ctx) {
    debug!("emscripten::nullFunc_viji");
}
pub fn nullFunc_vijiii(ctx: &mut Ctx) {
    debug!("emscripten::nullFunc_vijiii");
}
pub fn nullFunc_vijj(ctx: &mut Ctx) {
    debug!("emscripten::nullFunc_vijj");
}
pub fn invoke_dii(ctx: &mut Ctx) {
    debug!("emscripten::invoke_dii");
    if let Some(invoke_dii) = &get_emscripten_data(ctx).invoke_dii {
        invoke_dii.call().unwrap()
    } else {
        panic!("invoke_dii is set to None");
    }
}
pub fn invoke_diiii(ctx: &mut Ctx) {
    debug!("emscripten::invoke_diiii");
    if let Some(invoke_diiii) = &get_emscripten_data(ctx).invoke_diiii {
        invoke_diiii.call().unwrap()
    } else {
        panic!("invoke_diiii is set to None");
    }
}
pub fn invoke_iiiii(ctx: &mut Ctx) {
    debug!("emscripten::invoke_iiiii");
    if let Some(invoke_iiiii) = &get_emscripten_data(ctx).invoke_iiiii {
        invoke_iiiii.call().unwrap()
    } else {
        panic!("invoke_iiiii is set to None");
    }
}
pub fn invoke_iiiiii(ctx: &mut Ctx) {
    debug!("emscripten::invoke_iiiiii");
    if let Some(invoke_iiiiii) = &get_emscripten_data(ctx).invoke_iiiiii {
        invoke_iiiiii.call().unwrap()
    } else {
        panic!("invoke_iiiiii is set to None");
    }
}
pub fn invoke_vd(ctx: &mut Ctx) {
    debug!("emscripten::invoke_vd");
    if let Some(invoke_vd) = &get_emscripten_data(ctx).invoke_vd {
        invoke_vd.call().unwrap()
    } else {
        panic!("invoke_vd is set to None");
    }
}
pub fn invoke_viiiii(ctx: &mut Ctx) {
    debug!("emscripten::invoke_viiiii");
    if let Some(invoke_viiiii) = &get_emscripten_data(ctx).invoke_viiiii {
        invoke_viiiii.call().unwrap()
    } else {
        panic!("invoke_viiiii is set to None");
    }
}
pub fn invoke_viiiiii(ctx: &mut Ctx) {
    debug!("emscripten::invoke_viiiiii");
    if let Some(invoke_viiiiii) = &get_emscripten_data(ctx).invoke_viiiiii {
        invoke_viiiiii.call().unwrap()
    } else {
        panic!("invoke_viiiiii is set to None");
    }
}
pub fn invoke_viiiiiii(ctx: &mut Ctx) {
    debug!("emscripten::invoke_viiiiiii");
    if let Some(invoke_viiiiiii) = &get_emscripten_data(ctx).invoke_viiiiiii {
        invoke_viiiiiii.call().unwrap()
    } else {
        panic!("invoke_viiiiiii is set to None");
    }
}
pub fn invoke_viiiiiiii(ctx: &mut Ctx) {
    debug!("emscripten::invoke_viiiiiiii");
    if let Some(invoke_viiiiiiii) = &get_emscripten_data(ctx).invoke_viiiiiiii {
        invoke_viiiiiiii.call().unwrap()
    } else {
        panic!("invoke_viiiiiiii is set to None");
    }
}
pub fn invoke_viiiiiiiii(ctx: &mut Ctx) {
    debug!("emscripten::invoke_viiiiiiiii");
    if let Some(invoke_viiiiiiiii) = &get_emscripten_data(ctx).invoke_viiiiiiiii {
        invoke_viiiiiiiii.call().unwrap()
    } else {
        panic!("invoke_viiiiiiiii is set to None");
    }
}
pub fn invoke_iiji(ctx: &mut Ctx) {
    debug!("emscripten::invoke_iiji");
    if let Some(invoke_iiji) = &get_emscripten_data(ctx).invoke_iiji {
        invoke_iiji.call().unwrap()
    } else {
        panic!("invoke_iiji is set to None");
    }
}
pub fn invoke_j(ctx: &mut Ctx) {
    debug!("emscripten::invoke_j");
    if let Some(invoke_j) = &get_emscripten_data(ctx).invoke_j {
        invoke_j.call().unwrap()
    } else {
        panic!("invoke_j is set to None");
    }
}
pub fn invoke_ji(ctx: &mut Ctx) {
    debug!("emscripten::invoke_ji");
    if let Some(invoke_ji) = &get_emscripten_data(ctx).invoke_ji {
        invoke_ji.call().unwrap()
    } else {
        panic!("invoke_ji is set to None");
    }
}
pub fn invoke_jij(ctx: &mut Ctx) {
    debug!("emscripten::invoke_jij");
    if let Some(invoke_jij) = &get_emscripten_data(ctx).invoke_jij {
        invoke_jij.call().unwrap()
    } else {
        panic!("invoke_jij is set to None");
    }
}
pub fn invoke_jjj(ctx: &mut Ctx) {
    debug!("emscripten::invoke_jjj");
    if let Some(invoke_jjj) = &get_emscripten_data(ctx).invoke_jjj {
        invoke_jjj.call().unwrap()
    } else {
        panic!("invoke_jjj is set to None");
    }
}
pub fn invoke_viiij(ctx: &mut Ctx) {
    debug!("emscripten::invoke_viiij");
    if let Some(invoke_viiij) = &get_emscripten_data(ctx).invoke_viiij {
        invoke_viiij.call().unwrap()
    } else {
        panic!("invoke_viiij is set to None");
    }
}
pub fn invoke_viiijiiii(ctx: &mut Ctx) {
    debug!("emscripten::invoke_viiijiiii");
    if let Some(invoke_viiijiiii) = &get_emscripten_data(ctx).invoke_viiijiiii {
        invoke_viiijiiii.call().unwrap()
    } else {
        panic!("invoke_viiijiiii is set to None");
    }
}
pub fn invoke_viiijiiiiii(ctx: &mut Ctx) {
    debug!("emscripten::invoke_viiijiiiiii");
    if let Some(invoke_viiijiiiiii) = &get_emscripten_data(ctx).invoke_viiijiiiiii {
        invoke_viiijiiiiii.call().unwrap()
    } else {
        panic!("invoke_viiijiiiiii is set to None");
    }
}
pub fn invoke_viij(ctx: &mut Ctx) {
    debug!("emscripten::invoke_viij");
    if let Some(invoke_viij) = &get_emscripten_data(ctx).invoke_viij {
        invoke_viij.call().unwrap()
    } else {
        panic!("invoke_viij is set to None");
    }
}
pub fn invoke_viiji(ctx: &mut Ctx) {
    debug!("emscripten::invoke_viiji");
    if let Some(invoke_viiji) = &get_emscripten_data(ctx).invoke_viiji {
        invoke_viiji.call().unwrap()
    } else {
        panic!("invoke_viiji is set to None");
    }
}
pub fn invoke_viijiii(ctx: &mut Ctx) {
    debug!("emscripten::invoke_viijiii");
    if let Some(invoke_viijiii) = &get_emscripten_data(ctx).invoke_viijiii {
        invoke_viijiii.call().unwrap()
    } else {
        panic!("invoke_viijiii is set to None");
    }
}
pub fn invoke_viijj(ctx: &mut Ctx) {
    debug!("emscripten::invoke_viijj");
    if let Some(invoke_viijj) = &get_emscripten_data(ctx).invoke_viijj {
        invoke_viijj.call().unwrap()
    } else {
        panic!("invoke_viijj is set to None");
    }
}
pub fn invoke_vij(ctx: &mut Ctx) {
    debug!("emscripten::invoke_vij");
    if let Some(invoke_vij) = &get_emscripten_data(ctx).invoke_vij {
        invoke_vij.call().unwrap()
    } else {
        panic!("invoke_vij is set to None");
    }
}
pub fn invoke_viji(ctx: &mut Ctx) {
    debug!("emscripten::invoke_viji");
    if let Some(invoke_viji) = &get_emscripten_data(ctx).invoke_viji {
        invoke_viji.call().unwrap()
    } else {
        panic!("invoke_viji is set to None");
    }
}
pub fn invoke_vijiii(ctx: &mut Ctx) {
    debug!("emscripten::invoke_vijiii");
    if let Some(invoke_vijiii) = &get_emscripten_data(ctx).invoke_vijiii {
        invoke_vijiii.call().unwrap()
    } else {
        panic!("invoke_vijiii is set to None");
    }
}
pub fn invoke_vijj(ctx: &mut Ctx) {
    debug!("emscripten::invoke_vijj");
    if let Some(invoke_vijj) = &get_emscripten_data(ctx).invoke_vijj {
        invoke_vijj.call().unwrap()
    } else {
        panic!("invoke_vijj is set to None");
    }
}
