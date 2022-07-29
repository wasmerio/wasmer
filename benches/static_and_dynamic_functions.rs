use criterion::{black_box, criterion_group, criterion_main, Criterion};

use wasmer::*;

static BASIC_WAT: &str = r#"(module
    (func $multiply (import "env" "multiply") (param i32 i32) (result i32))
    (func (export "add") (param i32 i32) (result i32)
       (i32.add (local.get 0)
                (local.get 1)))
    (func (export "add20") (param i32 i32 i32 i32 i32
                                  i32 i32 i32 i32 i32
                                  i32 i32 i32 i32 i32
                                  i32 i32 i32 i32 i32) (result i32)
       (i32.add
                (i32.add 
                         (i32.add (i32.add (i32.add (local.get 0)  (local.get 1))
                                           (i32.add (local.get 2)  (local.get 3)))
                                  (i32.add (i32.add (local.get 4)  (local.get 5))
                                           (i32.add (local.get 6)  (local.get 7))))
                         (i32.add
                                  (i32.add (i32.add (local.get 8)  (local.get 9))
                                           (i32.add (local.get 10) (local.get 11)))
                                  (i32.add (i32.add (local.get 12) (local.get 13))
                                           (i32.add (local.get 14) (local.get 15)))))

                (i32.add (i32.add (local.get 16) (local.get 17))
                         (i32.add (local.get 18) (local.get 19))))
)
    (func (export "double_then_add") (param i32 i32) (result i32)
       (i32.add (call $multiply (local.get 0) (i32.const 2))
                (call $multiply (local.get 1) (i32.const 2))))
)"#;

pub fn run_basic_static_function(store: &Store, compiler_name: &str, c: &mut Criterion) {
    let module = Module::new(store, BASIC_WAT).unwrap();
    let import_object = imports! {
        "env" => {
            "multiply" => Function::new_native(store, |a: i32, b: i32| a * b),
        },
    };
    let instance = Instance::new(&module, &import_object).unwrap();
    let dyn_f: &Function = instance.exports.get("add").unwrap();
    let f: TypedFunction<(i32, i32), i32> = dyn_f.native().unwrap();

    c.bench_function(&format!("basic static func {}", compiler_name), |b| {
        b.iter(|| {
            let result = black_box(f.call(4, 6).unwrap());
            assert_eq!(result, 10);
        })
    });

    let dyn_f_many: &Function = instance.exports.get("add20").unwrap();
    let f_many: TypedFunction<
        (
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
        ),
        i32,
    > = dyn_f_many.native().unwrap();
    c.bench_function(
        &format!("basic static func with many args {}", compiler_name),
        |b| {
            b.iter(|| {
                let result = black_box(
                    f_many
                        .call(
                            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
                        )
                        .unwrap(),
                );
                assert_eq!(result, 210);
            })
        },
    );
}

pub fn run_basic_dynamic_function(store: &Store, compiler_name: &str, c: &mut Criterion) {
    let module = Module::new(store, BASIC_WAT).unwrap();
    let import_object = imports! {
        "env" => {
            "multiply" => Function::new_native(store, |a: i32, b: i32| a * b),
        },
    };
    let instance = Instance::new(&module, &import_object).unwrap();

    let dyn_f: &Function = instance.exports.get("add").unwrap();
    c.bench_function(&format!("basic dynfunc {}", compiler_name), |b| {
        b.iter(|| {
            let dyn_result = black_box(dyn_f.call(&[Val::I32(4), Val::I32(6)]).unwrap());
            assert_eq!(dyn_result[0], Val::I32(10));
        })
    });

    let dyn_f_many: &Function = instance.exports.get("add20").unwrap();
    c.bench_function(
        &format!("basic dynfunc with many args {}", compiler_name),
        |b| {
            b.iter(|| {
                let dyn_result = black_box(
                    dyn_f_many
                        .call(&[
                            Val::I32(1),
                            Val::I32(2),
                            Val::I32(3),
                            Val::I32(4),
                            Val::I32(5),
                            Val::I32(6),
                            Val::I32(7),
                            Val::I32(8),
                            Val::I32(9),
                            Val::I32(10),
                            Val::I32(11),
                            Val::I32(12),
                            Val::I32(13),
                            Val::I32(14),
                            Val::I32(15),
                            Val::I32(16),
                            Val::I32(17),
                            Val::I32(18),
                            Val::I32(19),
                            Val::I32(20),
                        ])
                        .unwrap(),
                );
                assert_eq!(dyn_result[0], Val::I32(210));
            })
        },
    );
}

fn run_static_benchmarks(_c: &mut Criterion) {
    #[cfg(feature = "llvm")]
    {
        let mut store = Store::new(wasmer_compiler_llvm::LLVM::new());
        run_basic_static_function(&store, "llvm", c);
    }

    #[cfg(feature = "cranelift")]
    {
        let mut store = Store::new(wasmer_compiler_cranelift::Cranelift::new());
        run_basic_static_function(&store, "cranelift", c);
    }

    #[cfg(feature = "singlepass")]
    {
        let mut store = Store::new(wasmer_compiler_singlepass::Singlepass::new());
        run_basic_static_function(&store, "singlepass", c);
    }
}

fn run_dynamic_benchmarks(_c: &mut Criterion) {
    #[cfg(feature = "llvm")]
    {
        let mut store = Store::new(wasmer_compiler_llvm::LLVM::new());
        run_basic_dynamic_function(&store, "llvm", c);
    }

    #[cfg(feature = "cranelift")]
    {
        let mut store = Store::new(wasmer_compiler_cranelift::Cranelift::new());
        run_basic_dynamic_function(&store, "cranelift", c);
    }

    #[cfg(feature = "singlepass")]
    {
        let mut store = Store::new(wasmer_compiler_singlepass::Singlepass::new());
        run_basic_dynamic_function(&store, "singlepass", c);
    }
}

criterion_group!(benches, run_static_benchmarks, run_dynamic_benchmarks);

criterion_main!(benches);
