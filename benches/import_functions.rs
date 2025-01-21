use criterion::{black_box, criterion_group, criterion_main, Criterion};

use wasmer::*;

pub fn run_import_inner(store: &mut Store, n_fn: u32, compiler_name: &str, c: &mut Criterion) {
    let donor_module = Module::new(
        store,
        format!(
            "(module {})",
            (0..n_fn)
                .map(|i| format!(
                    "(func (export \"f{i}\") (param {}) (result i32) i32.const 0)",
                    "i32 ".repeat(((i + 1) % 200) as usize)
                ))
                .collect::<Vec<String>>()
                .join("\n")
        ),
    )
    .unwrap();
    let donor_instance = Instance::new(store, &donor_module, &imports! {}).unwrap();
    let module = Module::new(
        store,
        format!(
            "(module {})",
            (0..n_fn)
                .map(|i| format!(
                    "(func (import \"env\" \"f{i}\") (param {}) (result i32))",
                    "i32 ".repeat(((i + 1) % 200) as usize),
                ))
                .collect::<Vec<String>>()
                .join("\n")
        ),
    )
    .unwrap();

    c.bench_function(&format!("import in {compiler_name} (size: {n_fn})"), |b| {
        let module = module.clone();
        b.iter(|| {
            let mut imports = imports! {};
            for i in 0..n_fn {
                let name = format!("f{i}");
                imports.define(
                    "env",
                    &name,
                    donor_instance.exports.get_function(&name).unwrap().clone(),
                );
            }

            let instance = black_box(Instance::new(store, &module, &imports));
            assert!(instance.is_ok());
        })
    });
}

fn run_import_functions_benchmarks_small(_c: &mut Criterion) {
    #[allow(unused_variables)]
    let size = 10;

    #[cfg(feature = "llvm")]
    {
        let mut store = Store::new(wasmer_compiler_llvm::LLVM::new());
        run_import_inner(&mut store, size, "cranelift", _c);
    }

    #[cfg(feature = "cranelift")]
    {
        let mut store = Store::new(wasmer_compiler_cranelift::Cranelift::new());
        run_import_inner(&mut store, size, "cranelift", _c);
    }

    #[cfg(feature = "singlepass")]
    {
        let mut store = Store::new(wasmer_compiler_singlepass::Singlepass::new());
        run_import_inner(&mut store, size, "cranelift", _c);
    }
}

fn run_import_functions_benchmarks_medium(_c: &mut Criterion) {
    #[allow(unused_variables)]
    let size = 100;

    #[cfg(feature = "llvm")]
    {
        let mut store = Store::new(wasmer_compiler_llvm::LLVM::new());
        run_import_inner(&mut store, size, "cranelift", _c);
    }

    #[cfg(feature = "cranelift")]
    {
        let mut store = Store::new(wasmer_compiler_cranelift::Cranelift::new());
        run_import_inner(&mut store, size, "cranelift", _c);
    }

    #[cfg(feature = "singlepass")]
    {
        let mut store = Store::new(wasmer_compiler_singlepass::Singlepass::new());
        run_import_inner(&mut store, size, "cranelift", _c);
    }
}

fn run_import_functions_benchmarks_large(_c: &mut Criterion) {
    #[allow(unused_variables)]
    let size = 1000;
    #[cfg(feature = "llvm")]
    {
        let mut store = Store::new(wasmer_compiler_llvm::LLVM::new());
        run_import_inner(&mut store, size, "cranelift", _c);
    }

    #[cfg(feature = "cranelift")]
    {
        let mut store = Store::new(wasmer_compiler_cranelift::Cranelift::new());
        run_import_inner(&mut store, size, "cranelift", _c);
    }

    #[cfg(feature = "singlepass")]
    {
        let mut store = Store::new(wasmer_compiler_singlepass::Singlepass::new());
        run_import_inner(&mut store, size, "cranelift", _c);
    }
}

criterion_group!(
    benches,
    run_import_functions_benchmarks_small,
    run_import_functions_benchmarks_medium,
    run_import_functions_benchmarks_large
);

criterion_main!(benches);
