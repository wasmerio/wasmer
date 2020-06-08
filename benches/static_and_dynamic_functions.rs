use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::sync::Arc;
use test_utils::{get_compiler_config_from_str, wasmer_compilers};
use wasmer::*;
use wasmer_engine_jit::JITEngine;

static BASIC_WAT: &str = r#"(module
    (func $multiply (import "env" "multiply") (param i32 i32) (result i32))
    (func (export "add") (param i32 i32) (result i32)
       (i32.add (local.get 0)
                (local.get 1)))
    (func (export "double_then_add") (param i32 i32) (result i32)
       (i32.add (call $multiply (local.get 0) (i32.const 2))
                (call $multiply (local.get 1) (i32.const 2))))
)"#;

wasmer_compilers! {
    use criterion::Criterion;
    use super::*;
    pub fn run_basic_static_function(c: &mut Criterion) {
        let store = get_store();
        let module = Module::new(&store, BASIC_WAT).unwrap();
        let import_object = imports! {
            "env" => {
                "multiply" => Function::new(&store, |a: i32, b: i32| a * b),
            },
        };
        let instance = Instance::new(&module, &import_object).unwrap();
        let dyn_f: Function = instance.exports.get("add").unwrap();
        let f: NativeFunc<(i32, i32), i32> = dyn_f.native().unwrap();

        c.bench_function(&format!("basic static func {}", COMPILER_NAME), |b| {
            b.iter(|| {
                let result = black_box(f.call(4, 6).unwrap());
                assert_eq!(result, 10);
            })
        });
    }

    pub fn run_basic_dynamic_function(c: &mut Criterion) {
        let store = get_store();
        let module = Module::new(&store, BASIC_WAT).unwrap();
        let import_object = imports! {
            "env" => {
                "multiply" => Function::new(&store, |a: i32, b: i32| a * b),
            },
        };
        let instance = Instance::new(&module, &import_object).unwrap();

        let dyn_f: Function = instance.exports.get("add").unwrap();
        c.bench_function(&format!("basic dynfunc {}", COMPILER_NAME), |b| {
            b.iter(|| {
                let dyn_result = black_box(dyn_f.call(&[Val::I32(4), Val::I32(6)]).unwrap());
                assert_eq!(dyn_result[0], Val::I32(10));
            })
        });
    }
}

fn run_static_benchmarks(c: &mut Criterion) {
    #[cfg(feature = "llvm")]
    llvm::run_basic_static_function(c);

    #[cfg(feature = "cranelift")]
    cranelift::run_basic_static_function(c);

    #[cfg(feature = "singlepass")]
    singlepass::run_basic_static_function(c);
}

fn run_dynamic_benchmarks(c: &mut Criterion) {
    #[cfg(feature = "llvm")]
    llvm::run_basic_dynamic_function(c);

    #[cfg(feature = "cranelift")]
    cranelift::run_basic_dynamic_function(c);

    #[cfg(feature = "singlepass")]
    singlepass::run_basic_dynamic_function(c);
}

criterion_group!(benches, run_static_benchmarks, run_dynamic_benchmarks);

criterion_main!(benches);
