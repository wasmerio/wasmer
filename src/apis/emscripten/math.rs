use crate::webassembly::Instance;

/// emscripten: _llvm_log10_f64
pub extern "C" fn _llvm_log10_f64(value: f64) -> f64 {
    debug!("emscripten::_llvm_log10_f64");
    value.log10()
}

/// emscripten: _llvm_log2_f64
pub extern "C" fn _llvm_log2_f64(value: f64) -> f64 {
    debug!("emscripten::_llvm_log2_f64");
    value.log2()
}

// emscripten: f64-rem
pub extern "C" fn f64_rem(x: f64, y: f64) -> f64 {
    debug!("emscripten::f64-rem");
    x % y
}
