extern crate libm;
extern crate wasmer_runtime_core;

use libm::fma;
use wasmer_runtime_core::vm::Ctx;

pub fn fma_f64(_ctx: &mut Ctx, x: f64, y: f64, z: f64) -> f64 {
    fma(x, y, z)
}
