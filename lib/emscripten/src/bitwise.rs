use crate::emscripten_target;
use wasmer_runtime_core::vm::Ctx;

///emscripten: _llvm_bswap_i64
pub fn _llvm_bswap_i64(_ctx: &mut Ctx, _low: i32, high: i32) -> i32 {
    debug!("emscripten::_llvm_bswap_i64");
    emscripten_target::setTempRet0(_ctx, _low.swap_bytes());
    high.swap_bytes()
}
