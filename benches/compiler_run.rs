use criterion::{criterion_group, criterion_main, Criterion};
use wasmer::{sys::*, *};

static BENCHMARKS_ARTIFACTS_BASE_URL: &str = "https://pub-53a226d993e144159d6f8b993fe0cbf3.r2.dev";

#[allow(unreachable_code)]
fn get_engine() -> Engine {
    #[cfg(feature = "llvm")]
    return LLVM::new().into();

    #[cfg(feature = "singlepass")]
    return Singlepass::new().into();

    #[cfg(feature = "cranelift")]
    return Cranelift::new().into();

    #[cfg(not(any(feature = "cranelift", feature = "llvm", feature = "singlepass")))]
    return Default::default();
}

pub fn run_fn(c: &mut Criterion, module: &[u8], name: &str, input: i64) {
    c.bench_function(name, |b| {
        let engine = get_engine();
        let mut store = Store::new(engine);
        let module = Module::new(&store, module).unwrap();
        let import_object = imports! {};
        let instance = Instance::new(&mut store, &module, &import_object).unwrap();
        let func = instance
            .exports
            .get_typed_function::<i64, i64>(&store, "run")
            .unwrap();

        b.iter(|| {
            _ = func.call(&mut store, input);
        })
    });
}

pub fn download_and_run(c: &mut Criterion) {
    let name = if cfg!(feature = "cranelift") {
        "cranelift"
    } else if cfg!(feature = "llvm") {
        "llvm"
    } else if cfg!(feature = "singlepass") {
        "singlepass"
    } else if cfg!(feature = "v8") {
        "v8"
    } else if cfg!(feature = "wamr") {
        "wamr"
    } else if cfg!(feature = "wasmi") {
        "wasmi"
    } else {
        panic!("Unrecognized backend!")
    };

    let bytes = include_bytes!("./mods/counter.wasm");
    run_fn(c, bytes, &format!("exec/{name}/counter"), 5_000_000);
    let bytes = include_bytes!("./mods/primes.wasm");
    run_fn(c, bytes, &format!("exec/{name}/primes"), 1_000);
    let bytes = include_bytes!("./mods/fib_rec.wasm");
    run_fn(c, bytes, &format!("exec/{name}/fib_rec"), 40);
    let bytes = include_bytes!("./mods/fib_iter.wasm");
    run_fn(c, bytes, &format!("exec/{name}/fib_iter"), 2_000_000);
    let bytes = include_bytes!("./mods/bulk_ops.wasm");
    run_fn(c, bytes, &format!("exec/{name}/bulk_ops"), 5_000);
    let bytes = include_bytes!("./mods/matmul.wasm");
    run_fn(c, bytes, &format!("exec/{name}/matmul"), 200);
    let bytes = include_bytes!("./mods/argon2.wasm");
    run_fn(c, bytes, &format!("exec/{name}/argon2"), 1);
}

criterion_group!(
    name = run_benches;
    config = Criterion::default().sample_size(60);
    targets = download_and_run
);

criterion_main!(run_benches);
