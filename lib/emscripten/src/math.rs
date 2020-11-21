use crate::EmEnv;

pub fn _llvm_copysign_f32(x: f64, y: f64) -> f64 {
    x.copysign(y)
}

pub fn _llvm_copysign_f64(x: f64, y: f64) -> f64 {
    x.copysign(y)
}

/// emscripten: _llvm_log10_f64
pub fn _llvm_log10_f64(value: f64) -> f64 {
    debug!("emscripten::_llvm_log10_f64");
    value.log10()
}

/// emscripten: _llvm_log2_f64
pub fn _llvm_log2_f64(value: f64) -> f64 {
    debug!("emscripten::_llvm_log2_f64");
    value.log2()
}

/// emscripten: _llvm_sin_f64
pub fn _llvm_sin_f64(value: f64) -> f64 {
    debug!("emscripten::_llvm_sin_f64");
    value.sin()
}

/// emscripten: _llvm_cos_f64
pub fn _llvm_cos_f64(value: f64) -> f64 {
    debug!("emscripten::_llvm_cos_f64");
    value.cos()
}

pub fn _llvm_log10_f32(_value: f64) -> f64 {
    debug!("emscripten::_llvm_log10_f32");
    -1.0
}

pub fn _llvm_log2_f32(_value: f64) -> f64 {
    debug!("emscripten::_llvm_log10_f32");
    -1.0
}

pub fn _llvm_exp2_f32(value: f32) -> f32 {
    debug!("emscripten::_llvm_exp2_f32");
    2f32.powf(value)
}

pub fn _llvm_exp2_f64(value: f64) -> f64 {
    debug!("emscripten::_llvm_exp2_f64");
    2f64.powf(value)
}

pub fn _llvm_trunc_f64(value: f64) -> f64 {
    debug!("emscripten::_llvm_trunc_f64");
    value.trunc()
}

pub fn _llvm_fma_f64(value: f64, a: f64, b: f64) -> f64 {
    debug!("emscripten::_llvm_fma_f64");
    value.mul_add(a, b)
}

pub fn _emscripten_random(_ctx: &EmEnv) -> f64 {
    debug!("emscripten::_emscripten_random");
    -1.0
}

// emscripten: asm2wasm.f64-rem
pub fn f64_rem(x: f64, y: f64) -> f64 {
    debug!("emscripten::f64-rem");
    x % y
}

// emscripten: global.Math pow
pub fn pow(x: f64, y: f64) -> f64 {
    x.powf(y)
}

// emscripten: global.Math exp
pub fn exp(value: f64) -> f64 {
    value.exp()
}

// emscripten: global.Math log
pub fn log(value: f64) -> f64 {
    value.ln()
}

// emscripten: global.Math sqrt
pub fn sqrt(value: f64) -> f64 {
    value.sqrt()
}

// emscripten: global.Math floor
pub fn floor(value: f64) -> f64 {
    value.floor()
}

// emscripten: global.Math fabs
pub fn fabs(value: f64) -> f64 {
    value.abs()
}

// emscripten: asm2wasm.f64-to-int
pub fn f64_to_int(value: f64) -> i32 {
    debug!("emscripten::f64_to_int {}", value);
    value as i32
}
