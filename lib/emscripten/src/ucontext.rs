use crate::EmEnv;
use wasmer::FunctionEnv;

pub fn _getcontext(mut _ctx: FunctionEnv<'_, EmEnv>, _ucp: i32) -> i32 {
    debug!("emscripten::_getcontext({})", _ucp);
    0
}
pub fn _makecontext(
    mut _ctx: FunctionEnv<'_, EmEnv>,
    _ucp: i32,
    _func: i32,
    _argc: i32,
    _argv: i32,
) {
    debug!(
        "emscripten::_makecontext({}, {}, {}, {})",
        _ucp, _func, _argc, _argv
    );
}
pub fn _setcontext(mut _ctx: FunctionEnv<'_, EmEnv>, _ucp: i32) -> i32 {
    debug!("emscripten::_setcontext({})", _ucp);
    0
}
pub fn _swapcontext(mut _ctx: FunctionEnv<'_, EmEnv>, _oucp: i32, _ucp: i32) -> i32 {
    debug!("emscripten::_swapcontext({}, {})", _oucp, _ucp);
    0
}
