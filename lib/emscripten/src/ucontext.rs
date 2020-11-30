use crate::EmEnv;

pub fn _getcontext(_ctx: &EmEnv, _ucp: i32) -> i32 {
    debug!("emscripten::_getcontext({})", _ucp);
    0
}
pub fn _makecontext(_ctx: &EmEnv, _ucp: i32, _func: i32, _argc: i32, _argv: i32) {
    debug!(
        "emscripten::_makecontext({}, {}, {}, {})",
        _ucp, _func, _argc, _argv
    );
}
pub fn _setcontext(_ctx: &EmEnv, _ucp: i32) -> i32 {
    debug!("emscripten::_setcontext({})", _ucp);
    0
}
pub fn _swapcontext(_ctx: &EmEnv, _oucp: i32, _ucp: i32) -> i32 {
    debug!("emscripten::_swapcontext({}, {})", _oucp, _ucp);
    0
}
