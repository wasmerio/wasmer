use wasmer_runtime_core::vm::Ctx;

/// emscripten: _llvm_log10_f64
pub extern "C" fn _llvm_log10_f64(value: f64, _ctx: &mut Ctx) -> f64 {
    debug!("emscripten::_llvm_log10_f64");
    value.log10()
}

/// emscripten: _llvm_log2_f64
pub extern "C" fn _llvm_log2_f64(value: f64, _ctx: &mut Ctx) -> f64 {
    debug!("emscripten::_llvm_log2_f64");
    value.log2()
}

pub extern "C" fn _llvm_log10_f32(_value: f64, _ctx: &mut Ctx) -> f64 {
    debug!("emscripten::_llvm_log10_f32");
    -1.0
}

pub extern "C" fn _llvm_log2_f32(_value: f64, _ctx: &mut Ctx) -> f64 {
    debug!("emscripten::_llvm_log10_f32");
    -1.0
}

pub extern "C" fn _emscripten_random(_ctx: &mut Ctx) -> f64 {
    debug!("emscripten::_emscripten_random");
    -1.0
}

// emscripten: f64-rem
pub extern "C" fn f64_rem(x: f64, y: f64, _ctx: &mut Ctx) -> f64 {
    debug!("emscripten::f64-rem");
    x % y
}

// emscripten: global.Math pow
pub extern "C" fn pow(x: f64, y: f64, _ctx: &mut Ctx) -> f64 {
    x.powf(y)
}
