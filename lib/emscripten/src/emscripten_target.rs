#![allow(non_snake_case)]

use crate::env::{get_emscripten_data, get_emscripten_funcs};
use crate::EmEnv;
#[cfg(target_os = "linux")]
use libc::getdtablesize;
use wasmer::FunctionEnvMut;

pub fn asm_const_i(_ctx: FunctionEnvMut<EmEnv>, _val: i32) -> i32 {
    debug!("emscripten::asm_const_i: {}", _val);
    0
}

pub fn exit_with_live_runtime(_ctx: FunctionEnvMut<EmEnv>) {
    debug!("emscripten::exit_with_live_runtime");
}

pub fn setTempRet0(ctx: FunctionEnvMut<EmEnv>, val: i32) {
    trace!("emscripten::setTempRet0: {}", val);
    get_emscripten_data(&ctx).as_mut().unwrap().temp_ret_0 = val;
}

pub fn getTempRet0(ctx: FunctionEnvMut<EmEnv>) -> i32 {
    trace!("emscripten::getTempRet0");
    get_emscripten_data(&ctx).as_ref().unwrap().temp_ret_0
}

pub fn _alarm(_ctx: FunctionEnvMut<EmEnv>, _seconds: u32) -> i32 {
    debug!("emscripten::_alarm({})", _seconds);
    0
}

pub fn _atexit(_ctx: FunctionEnvMut<EmEnv>, _func: i32) -> i32 {
    debug!("emscripten::_atexit");
    // TODO: implement atexit properly
    // __ATEXIT__.unshift({
    //     func: func,
    //     arg: arg
    // });
    0
}
pub fn __Unwind_Backtrace(_ctx: FunctionEnvMut<EmEnv>, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::__Unwind_Backtrace");
    0
}
pub fn __Unwind_FindEnclosingFunction(_ctx: FunctionEnvMut<EmEnv>, _a: i32) -> i32 {
    debug!("emscripten::__Unwind_FindEnclosingFunction");
    0
}
pub fn __Unwind_GetIPInfo(_ctx: FunctionEnvMut<EmEnv>, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::__Unwind_GetIPInfo");
    0
}
pub fn ___cxa_find_matching_catch_2(_ctx: FunctionEnvMut<EmEnv>) -> i32 {
    debug!("emscripten::___cxa_find_matching_catch_2");
    0
}
pub fn ___cxa_find_matching_catch_3(_ctx: FunctionEnvMut<EmEnv>, _a: i32) -> i32 {
    debug!("emscripten::___cxa_find_matching_catch_3");
    0
}
pub fn ___cxa_free_exception(_ctx: FunctionEnvMut<EmEnv>, _a: i32) {
    debug!("emscripten::___cxa_free_exception");
}
pub fn ___resumeException(_ctx: FunctionEnvMut<EmEnv>, _a: i32) {
    debug!("emscripten::___resumeException");
}
pub fn _dladdr(_ctx: FunctionEnvMut<EmEnv>, _a: i32, _b: i32) -> i32 {
    debug!("emscripten::_dladdr");
    0
}
pub fn ___gxx_personality_v0(
    _ctx: FunctionEnvMut<EmEnv>,
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
pub fn _getdtablesize(_ctx: FunctionEnvMut<EmEnv>) -> i32 {
    debug!("emscripten::getdtablesize");
    unsafe { getdtablesize() }
}
#[cfg(not(target_os = "linux"))]
pub fn _getdtablesize(_ctx: FunctionEnvMut<EmEnv>) -> i32 {
    debug!("emscripten::getdtablesize");
    -1
}
pub fn _gethostbyaddr(_ctx: FunctionEnvMut<EmEnv>, _addr: i32, _addrlen: i32, _atype: i32) -> i32 {
    debug!("emscripten::gethostbyaddr");
    0
}
pub fn _gethostbyname(_ctx: FunctionEnvMut<EmEnv>, _name: i32) -> i32 {
    debug!("emscripten::gethostbyname_r");
    0
}
pub fn _gethostbyname_r(
    _ctx: FunctionEnvMut<EmEnv>,
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
pub fn _getloadavg(_ctx: FunctionEnvMut<EmEnv>, _loadavg: i32, _nelem: i32) -> i32 {
    debug!("emscripten::getloadavg");
    0
}
#[allow(clippy::too_many_arguments)]
pub fn _getnameinfo(
    _ctx: FunctionEnvMut<EmEnv>,
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
    ($ctx: ident, $name:ident, $name_ref:ident, $( $arg:ident ),*) => {{
        let funcs = get_emscripten_funcs(&$ctx).clone();
        let sp = funcs.stack_save_ref().expect("stack_save is None").call(&mut $ctx).expect("stack_save call failed");
        let call = funcs.$name_ref().expect(concat!("Dynamic call is None: ", stringify!($name))).clone();
        match call.call(&mut $ctx, $($arg),*) {
            Ok(v) => v,
            Err(_e) => {
                let stack = funcs.stack_restore_ref().expect("stack_restore is None");
                stack.call(&mut $ctx, sp).expect("stack_restore call failed");
                // TODO: We should check if _e != "longjmp" and if that's the case, re-throw the error
                // JS version is: if (e !== e+0 && e !== 'longjmp') throw e;
                let threw = funcs.set_threw_ref().expect("set_threw is None");
                threw.call(&mut $ctx, 1, 0).expect("set_threw call failed");
                0 as _
            }
        }
    }};
}
macro_rules! invoke_no_return {
    ($ctx: ident, $name:ident, $name_ref:ident, $( $arg:ident ),*) => {{
        let funcs = get_emscripten_funcs(&$ctx).clone();
        let stack = funcs.stack_save_ref().expect("stack_save is None");
        let sp = stack.call(&mut $ctx).expect("stack_save call failed");
        let call = funcs.$name_ref().expect(concat!("Dynamic call is None: ", stringify!($name))).clone();
        match call.call(&mut $ctx, $($arg),*) {
            Ok(v) => v,
            Err(_e) => {
                let stack = funcs.stack_restore_ref().expect("stack_restore is None");
                stack.call(&mut $ctx, sp).expect("stack_restore call failed");
                // TODO: We should check if _e != "longjmp" and if that's the case, re-throw the error
                // JS version is: if (e !== e+0 && e !== 'longjmp') throw e;
                let threw = funcs.set_threw_ref().expect("set_threw is None");
                threw.call(&mut $ctx, 1, 0).expect("set_threw call failed");
            }
        }
    }};
}
// The invoke_j functions do not save the stack
macro_rules! invoke_no_stack_save {
    ($ctx: ident, $name:ident, $name_ref:ident, $( $arg:ident ),*) => {{
        let funcs = get_emscripten_funcs(&$ctx).clone();
        let call = funcs.$name_ref().expect(concat!(stringify!($name), " is set to None")).clone();

        call.call(&mut $ctx, $($arg),*).unwrap()
    }}
}

// Invoke functions
pub fn invoke_i(mut ctx: FunctionEnvMut<EmEnv>, index: i32) -> i32 {
    debug!("emscripten::invoke_i");
    invoke!(ctx, dyn_call_i, dyn_call_i_ref, index)
}
pub fn invoke_ii(mut ctx: FunctionEnvMut<EmEnv>, index: i32, a1: i32) -> i32 {
    debug!("emscripten::invoke_ii");
    invoke!(ctx, dyn_call_ii, dyn_call_ii_ref, index, a1)
}
pub fn invoke_iii(mut ctx: FunctionEnvMut<EmEnv>, index: i32, a1: i32, a2: i32) -> i32 {
    debug!("emscripten::invoke_iii");
    invoke!(ctx, dyn_call_iii, dyn_call_iii_ref, index, a1, a2)
}
pub fn invoke_iiii(mut ctx: FunctionEnvMut<EmEnv>, index: i32, a1: i32, a2: i32, a3: i32) -> i32 {
    debug!("emscripten::invoke_iiii");
    invoke!(ctx, dyn_call_iiii, dyn_call_iiii_ref, index, a1, a2, a3)
}
pub fn invoke_iifi(mut ctx: FunctionEnvMut<EmEnv>, index: i32, a1: i32, a2: f64, a3: i32) -> i32 {
    debug!("emscripten::invoke_iifi");
    invoke!(ctx, dyn_call_iifi, dyn_call_iifi_ref, index, a1, a2, a3)
}
pub fn invoke_v(mut ctx: FunctionEnvMut<EmEnv>, index: i32) {
    debug!("emscripten::invoke_v");
    invoke_no_return!(ctx, dyn_call_v, dyn_call_v_ref, index);
}
pub fn invoke_vi(mut ctx: FunctionEnvMut<EmEnv>, index: i32, a1: i32) {
    debug!("emscripten::invoke_vi");
    invoke_no_return!(ctx, dyn_call_vi, dyn_call_vi_ref, index, a1);
}
pub fn invoke_vii(mut ctx: FunctionEnvMut<EmEnv>, index: i32, a1: i32, a2: i32) {
    debug!("emscripten::invoke_vii");
    invoke_no_return!(ctx, dyn_call_vii, dyn_call_vii_ref, index, a1, a2);
}

pub fn invoke_viii(mut ctx: FunctionEnvMut<EmEnv>, index: i32, a1: i32, a2: i32, a3: i32) {
    debug!("emscripten::invoke_viii");
    invoke_no_return!(ctx, dyn_call_viii, dyn_call_viii_ref, index, a1, a2, a3);
}
pub fn invoke_viiii(
    mut ctx: FunctionEnvMut<EmEnv>,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
) {
    debug!("emscripten::invoke_viiii");
    invoke_no_return!(
        ctx,
        dyn_call_viiii,
        dyn_call_viiii_ref,
        index,
        a1,
        a2,
        a3,
        a4
    );
}
pub fn invoke_dii(mut ctx: FunctionEnvMut<EmEnv>, index: i32, a1: i32, a2: i32) -> f64 {
    debug!("emscripten::invoke_dii");
    invoke!(ctx, dyn_call_dii, dyn_call_dii_ref, index, a1, a2)
}
pub fn invoke_diiii(
    mut ctx: FunctionEnvMut<EmEnv>,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
) -> f64 {
    debug!("emscripten::invoke_diiii");
    invoke!(
        ctx,
        dyn_call_diiii,
        dyn_call_diiii_ref,
        index,
        a1,
        a2,
        a3,
        a4
    )
}
pub fn invoke_iiiii(
    mut ctx: FunctionEnvMut<EmEnv>,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
) -> i32 {
    debug!("emscripten::invoke_iiiii");
    invoke!(
        ctx,
        dyn_call_iiiii,
        dyn_call_iiiii_ref,
        index,
        a1,
        a2,
        a3,
        a4
    )
}
pub fn invoke_iiiiii(
    mut ctx: FunctionEnvMut<EmEnv>,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
) -> i32 {
    debug!("emscripten::invoke_iiiiii");
    invoke!(
        ctx,
        dyn_call_iiiiii,
        dyn_call_iiiiii_ref,
        index,
        a1,
        a2,
        a3,
        a4,
        a5
    )
}
#[allow(clippy::too_many_arguments)]
pub fn invoke_iiiiiii(
    mut ctx: FunctionEnvMut<EmEnv>,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
    a6: i32,
) -> i32 {
    debug!("emscripten::invoke_iiiiiii");
    invoke!(
        ctx,
        dyn_call_iiiiiii,
        dyn_call_iiiiiii_ref,
        index,
        a1,
        a2,
        a3,
        a4,
        a5,
        a6
    )
}
#[allow(clippy::too_many_arguments)]
pub fn invoke_iiiiiiii(
    mut ctx: FunctionEnvMut<EmEnv>,
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
    invoke!(
        ctx,
        dyn_call_iiiiiiii,
        dyn_call_iiiiiiii_ref,
        index,
        a1,
        a2,
        a3,
        a4,
        a5,
        a6,
        a7
    )
}
#[allow(clippy::too_many_arguments)]
pub fn invoke_iiiiiiiii(
    mut ctx: FunctionEnvMut<EmEnv>,
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
        dyn_call_iiiiiiiii_ref,
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
#[allow(clippy::too_many_arguments)]
pub fn invoke_iiiiiiiiii(
    mut ctx: FunctionEnvMut<EmEnv>,
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
        dyn_call_iiiiiiiiii_ref,
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
#[allow(clippy::too_many_arguments)]
pub fn invoke_iiiiiiiiiii(
    mut ctx: FunctionEnvMut<EmEnv>,
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
        dyn_call_iiiiiiiiiii_ref,
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
pub fn invoke_vd(mut ctx: FunctionEnvMut<EmEnv>, index: i32, a1: f64) {
    debug!("emscripten::invoke_vd");
    invoke_no_return!(ctx, dyn_call_vd, dyn_call_vd_ref, index, a1)
}
pub fn invoke_viiiii(
    mut ctx: FunctionEnvMut<EmEnv>,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
) {
    debug!("emscripten::invoke_viiiii");
    invoke_no_return!(
        ctx,
        dyn_call_viiiii,
        dyn_call_viiiii_ref,
        index,
        a1,
        a2,
        a3,
        a4,
        a5
    )
}
#[allow(clippy::too_many_arguments)]
pub fn invoke_viiiiii(
    mut ctx: FunctionEnvMut<EmEnv>,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
    a6: i32,
) {
    debug!("emscripten::invoke_viiiiii");
    invoke_no_return!(
        ctx,
        dyn_call_viiiiii,
        dyn_call_viiiiii_ref,
        index,
        a1,
        a2,
        a3,
        a4,
        a5,
        a6
    )
}
#[allow(clippy::too_many_arguments)]
pub fn invoke_viiiiiii(
    mut ctx: FunctionEnvMut<EmEnv>,
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
    invoke_no_return!(
        ctx,
        dyn_call_viiiiiii,
        dyn_call_viiiiiii_ref,
        index,
        a1,
        a2,
        a3,
        a4,
        a5,
        a6,
        a7
    )
}
#[allow(clippy::too_many_arguments)]
pub fn invoke_viiiiiiii(
    mut ctx: FunctionEnvMut<EmEnv>,
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
        dyn_call_viiiiiiii_ref,
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
#[allow(clippy::too_many_arguments)]
pub fn invoke_viiiiiiiii(
    mut ctx: FunctionEnvMut<EmEnv>,
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
        dyn_call_viiiiiiiii_ref,
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
#[allow(clippy::too_many_arguments)]
pub fn invoke_viiiiiiiiii(
    mut ctx: FunctionEnvMut<EmEnv>,
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
        dyn_call_viiiiiiiiii_ref,
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

pub fn invoke_iij(mut ctx: FunctionEnvMut<EmEnv>, index: i32, a1: i32, a2: i32, a3: i32) -> i32 {
    debug!("emscripten::invoke_iij");
    invoke!(ctx, dyn_call_iij, dyn_call_iij_ref, index, a1, a2, a3)
}

pub fn invoke_iji(mut ctx: FunctionEnvMut<EmEnv>, index: i32, a1: i32, a2: i32, a3: i32) -> i32 {
    debug!("emscripten::invoke_iji");
    invoke!(ctx, dyn_call_iji, dyn_call_iji_ref, index, a1, a2, a3)
}

pub fn invoke_iiji(
    mut ctx: FunctionEnvMut<EmEnv>,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
) -> i32 {
    debug!("emscripten::invoke_iiji");
    invoke!(ctx, dyn_call_iiji, dyn_call_iiji_ref, index, a1, a2, a3, a4)
}

#[allow(clippy::too_many_arguments)]
pub fn invoke_iiijj(
    mut ctx: FunctionEnvMut<EmEnv>,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
    a6: i32,
) -> i32 {
    debug!("emscripten::invoke_iiijj");
    invoke!(
        ctx,
        dyn_call_iiijj,
        dyn_call_iiijj_ref,
        index,
        a1,
        a2,
        a3,
        a4,
        a5,
        a6
    )
}
pub fn invoke_j(mut ctx: FunctionEnvMut<EmEnv>, index: i32) -> i32 {
    debug!("emscripten::invoke_j");
    invoke_no_stack_save!(ctx, dyn_call_j, dyn_call_j_ref, index)
}
pub fn invoke_ji(mut ctx: FunctionEnvMut<EmEnv>, index: i32, a1: i32) -> i32 {
    debug!("emscripten::invoke_ji");
    invoke_no_stack_save!(ctx, dyn_call_ji, dyn_call_ji_ref, index, a1)
}
pub fn invoke_jii(mut ctx: FunctionEnvMut<EmEnv>, index: i32, a1: i32, a2: i32) -> i32 {
    debug!("emscripten::invoke_jii");
    invoke_no_stack_save!(ctx, dyn_call_jii, dyn_call_jii_ref, index, a1, a2)
}

pub fn invoke_jij(mut ctx: FunctionEnvMut<EmEnv>, index: i32, a1: i32, a2: i32, a3: i32) -> i32 {
    debug!("emscripten::invoke_jij");
    invoke_no_stack_save!(ctx, dyn_call_jij, dyn_call_jij_ref, index, a1, a2, a3)
}
pub fn invoke_jjj(
    mut ctx: FunctionEnvMut<EmEnv>,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
) -> i32 {
    debug!("emscripten::invoke_jjj");
    invoke_no_stack_save!(ctx, dyn_call_jjj, dyn_call_jjj_ref, index, a1, a2, a3, a4)
}
pub fn invoke_viiij(
    mut ctx: FunctionEnvMut<EmEnv>,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
) {
    debug!("emscripten::invoke_viiij");
    invoke_no_stack_save!(
        ctx,
        dyn_call_viiij,
        dyn_call_viiij_ref,
        index,
        a1,
        a2,
        a3,
        a4,
        a5
    )
}
#[allow(clippy::too_many_arguments)]
pub fn invoke_viiijiiii(
    mut ctx: FunctionEnvMut<EmEnv>,
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
    invoke_no_stack_save!(
        ctx,
        dyn_call_viiijiiii,
        dyn_call_viiijiiii_ref,
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
#[allow(clippy::too_many_arguments)]
pub fn invoke_viiijiiiiii(
    mut ctx: FunctionEnvMut<EmEnv>,
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
    invoke_no_stack_save!(
        ctx,
        dyn_call_viiijiiiiii,
        dyn_call_viiijiiiiii_ref,
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
        a10,
        a11
    )
}
pub fn invoke_viij(mut ctx: FunctionEnvMut<EmEnv>, index: i32, a1: i32, a2: i32, a3: i32, a4: i32) {
    debug!("emscripten::invoke_viij");
    invoke_no_stack_save!(ctx, dyn_call_viij, dyn_call_viij_ref, index, a1, a2, a3, a4)
}
pub fn invoke_viiji(
    mut ctx: FunctionEnvMut<EmEnv>,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
) {
    debug!("emscripten::invoke_viiji");
    invoke_no_stack_save!(
        ctx,
        dyn_call_viiji,
        dyn_call_viiji_ref,
        index,
        a1,
        a2,
        a3,
        a4,
        a5
    )
}
#[allow(clippy::too_many_arguments)]
pub fn invoke_viijiii(
    mut ctx: FunctionEnvMut<EmEnv>,
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
    invoke_no_stack_save!(
        ctx,
        dyn_call_viijiii,
        dyn_call_viijiii_ref,
        index,
        a1,
        a2,
        a3,
        a4,
        a5,
        a6,
        a7
    )
}
#[allow(clippy::too_many_arguments)]
pub fn invoke_viijj(
    mut ctx: FunctionEnvMut<EmEnv>,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
    a6: i32,
) {
    debug!("emscripten::invoke_viijj");
    invoke_no_stack_save!(
        ctx,
        dyn_call_viijj,
        dyn_call_viijj_ref,
        index,
        a1,
        a2,
        a3,
        a4,
        a5,
        a6
    )
}
pub fn invoke_vj(mut ctx: FunctionEnvMut<EmEnv>, index: i32, a1: i32, a2: i32) {
    debug!("emscripten::invoke_vj");
    invoke_no_stack_save!(ctx, dyn_call_vj, dyn_call_vj_ref, index, a1, a2)
}
pub fn invoke_vjji(
    mut ctx: FunctionEnvMut<EmEnv>,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
) {
    debug!("emscripten::invoke_vjji");
    invoke_no_return!(
        ctx,
        dyn_call_vjji,
        dyn_call_vjji_ref,
        index,
        a1,
        a2,
        a3,
        a4,
        a5
    )
}
pub fn invoke_vij(mut ctx: FunctionEnvMut<EmEnv>, index: i32, a1: i32, a2: i32, a3: i32) {
    debug!("emscripten::invoke_vij");
    invoke_no_stack_save!(ctx, dyn_call_vij, dyn_call_vij_ref, index, a1, a2, a3)
}
pub fn invoke_viji(mut ctx: FunctionEnvMut<EmEnv>, index: i32, a1: i32, a2: i32, a3: i32, a4: i32) {
    debug!("emscripten::invoke_viji");
    invoke_no_stack_save!(ctx, dyn_call_viji, dyn_call_viji_ref, index, a1, a2, a3, a4)
}
#[allow(clippy::too_many_arguments)]
pub fn invoke_vijiii(
    mut ctx: FunctionEnvMut<EmEnv>,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
    a6: i32,
) {
    debug!("emscripten::invoke_vijiii");
    invoke_no_stack_save!(
        ctx,
        dyn_call_vijiii,
        dyn_call_vijiii_ref,
        index,
        a1,
        a2,
        a3,
        a4,
        a5,
        a6
    )
}
pub fn invoke_vijj(
    mut ctx: FunctionEnvMut<EmEnv>,
    index: i32,
    a1: i32,
    a2: i32,
    a3: i32,
    a4: i32,
    a5: i32,
) {
    debug!("emscripten::invoke_vijj");
    invoke_no_stack_save!(
        ctx,
        dyn_call_vijj,
        dyn_call_vijj_ref,
        index,
        a1,
        a2,
        a3,
        a4,
        a5
    )
}
pub fn invoke_vidd(mut ctx: FunctionEnvMut<EmEnv>, index: i32, a1: i32, a2: f64, a3: f64) {
    debug!("emscripten::invoke_viid");
    invoke_no_return!(ctx, dyn_call_vidd, dyn_call_vidd_ref, index, a1, a2, a3);
}
pub fn invoke_viid(mut ctx: FunctionEnvMut<EmEnv>, index: i32, a1: i32, a2: i32, a3: f64) {
    debug!("emscripten::invoke_viid");
    invoke_no_return!(ctx, dyn_call_viid, dyn_call_viid_ref, index, a1, a2, a3);
}
pub fn invoke_viidii(
    mut ctx: FunctionEnvMut<EmEnv>,
    index: i32,
    a1: i32,
    a2: i32,
    a3: f64,
    a4: i32,
    a5: i32,
) {
    debug!("emscripten::invoke_viidii");
    invoke_no_return!(
        ctx,
        dyn_call_viidii,
        dyn_call_viidii_ref,
        index,
        a1,
        a2,
        a3,
        a4,
        a5
    );
}
#[allow(clippy::too_many_arguments)]
pub fn invoke_viidddddddd(
    mut ctx: FunctionEnvMut<EmEnv>,
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
        dyn_call_viidddddddd_ref,
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
