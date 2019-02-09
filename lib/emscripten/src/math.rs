use wasmer_runtime_core::vm::Ctx;

/// emscripten: _llvm_log10_f64
pub fn _llvm_log10_f64(_ctx: &mut Ctx, value: f64) -> f64 {
    debug!("emscripten::_llvm_log10_f64");
    value.log10()
}

/// emscripten: _llvm_log2_f64
pub fn _llvm_log2_f64(_ctx: &mut Ctx, value: f64) -> f64 {
    debug!("emscripten::_llvm_log2_f64");
    value.log2()
}

pub fn _llvm_log10_f32(_ctx: &mut Ctx, _value: f64) -> f64 {
    debug!("emscripten::_llvm_log10_f32");
    -1.0
}

pub fn _llvm_log2_f32(_ctx: &mut Ctx, _value: f64) -> f64 {
    debug!("emscripten::_llvm_log10_f32");
    -1.0
}

pub fn _emscripten_random(_ctx: &mut Ctx) -> f64 {
    debug!("emscripten::_emscripten_random");
    -1.0
}

// emscripten: f64-rem
pub fn f64_rem(_ctx: &mut Ctx, x: f64, y: f64) -> f64 {
    debug!("emscripten::f64-rem");
    x % y
}

// emscripten: global.Math pow
pub fn pow(_ctx: &mut Ctx, x: f64, y: f64) -> f64 {
    x.powf(y)
}
