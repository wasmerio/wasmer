//! Singlepass on x86-64 without AVX: the SSE4.2 emitter must give the same float results.
#![cfg(all(feature = "sys", feature = "singlepass", target_arch = "x86_64"))]

use wasmer::sys::{CpuFeature, EngineBuilder, Singlepass, Target, Triple};
use wasmer::{Instance, Module, Store, Value, imports};

const CASES: &[(&str, u64)] = &[
    (
        "f64.const 7.5 f64.const 2.5 f64.add i64.reinterpret_f64",
        0x4024000000000000,
    ),
    (
        "f64.const 7.5 f64.const 2.5 f64.sub i64.reinterpret_f64",
        0x4014000000000000,
    ),
    (
        "f64.const 7.5 f64.const 2.5 f64.div i64.reinterpret_f64",
        0x4008000000000000,
    ),
    (
        "f64.const 7.5 f64.const 2.5 f64.min i64.reinterpret_f64",
        0x4004000000000000,
    ),
    (
        "f64.const 7.5 f64.const 2.5 f64.max i64.reinterpret_f64",
        0x401e000000000000,
    ),
    (
        "f64.const 7.5 f64.sqrt i64.reinterpret_f64",
        0x4005e8add236a58f,
    ),
    (
        "f64.const 7.5 f64.floor i64.reinterpret_f64",
        0x401c000000000000,
    ),
    (
        "f64.const 7.5 f64.ceil i64.reinterpret_f64",
        0x4020000000000000,
    ),
    (
        "f64.const 7.5 f64.trunc i64.reinterpret_f64",
        0x401c000000000000,
    ),
    (
        "f64.const 2.5 f64.nearest i64.reinterpret_f64",
        0x4000000000000000,
    ),
    (
        "f32.const 7.5 f32.const 2.5 f32.sub i32.reinterpret_f32 i64.extend_i32_u",
        0x40a00000,
    ),
    (
        "f32.const 7.5 f32.const 2.5 f32.min i32.reinterpret_f32 i64.extend_i32_u",
        0x40200000,
    ),
    (
        "f32.const 7.5 f32.nearest i32.reinterpret_f32 i64.extend_i32_u",
        0x41000000,
    ),
    ("f64.const nan f64.const 1.5 f64.gt i64.extend_i32_u", 0),
    ("f64.const nan f64.const 1.5 f64.ge i64.extend_i32_u", 0),
    ("f32.const nan f32.const 1.5 f32.gt i64.extend_i32_u", 0),
    ("f64.const 2.5 f64.const 1.5 f64.ge i64.extend_i32_u", 1),
    ("f64.const 1.5 f64.const 1.5 f64.ge i64.extend_i32_u", 1),
    ("f64.const 1.5 f64.const 1.5 f64.gt i64.extend_i32_u", 0),
    ("f32.const 0 f32.const -0 f32.gt i64.extend_i32_u", 0),
    ("f32.const -0 f32.const 0 f32.ge i64.extend_i32_u", 1),
    ("f64.const 1.5 i64.trunc_sat_f64_s", 1),
    ("f64.const 3.5e44 i64.trunc_sat_f64_s", 0x7fffffffffffffff),
    ("f64.const nan i64.trunc_sat_f64_s", 0),
    ("f64.const 7.5 i32.trunc_sat_f64_u i64.extend_i32_u", 7),
    ("f32.const 7.5 i64.trunc_sat_f32_u", 7),
    ("f32.const 1.5 i64.trunc_sat_f32_s", 1),
    ("f64.const 1.5 i64.trunc_f64_s", 1),
    ("f32.const 2.5 i32.trunc_f32_u i64.extend_i32_u", 2),
];

#[test]
fn sse42_float_ops_match_spec() {
    if !CpuFeature::for_host().contains(CpuFeature::SSE42) {
        return;
    }
    let features = CpuFeature::SSE2
        | CpuFeature::SSE3
        | CpuFeature::SSSE3
        | CpuFeature::SSE41
        | CpuFeature::SSE42;
    let mut wrong = vec![];
    for canonicalize_nans in [false, true] {
        for (body, want) in CASES {
            let mut compiler = Singlepass::new();
            compiler.canonicalize_nans(canonicalize_nans);
            let engine = EngineBuilder::new(compiler)
                .set_target(Some(Target::new(Triple::host(), features)))
                .engine();
            let mut store = Store::new(engine);
            let wat = format!("(module (func (export \"f\") (result i64) {body}))");
            let module = Module::new(&store, wat).unwrap();
            let instance = Instance::new(&mut store, &module, &imports! {}).unwrap();
            let f = instance.exports.get_function("f").unwrap();
            let got = f.call(&mut store, &[]).map(|v| v[0].clone());
            if !matches!(got, Ok(Value::I64(x)) if x as u64 == *want) {
                wrong.push(format!(
                    "canonicalize_nans={canonicalize_nans} {body}: {got:?}"
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}
