use criterion::{
    criterion_group, criterion_main, measurement::WallTime, BenchmarkGroup, Criterion,
};
use wasmer::*;

static BENCHMARKS_ARTIFACTS_BASE_URL: &str = "https://pub-083d1a0568d446d1aa5b2e07bd16983b.r2.dev";

#[allow(unreachable_code)]
fn get_engine() -> Engine {
    #[cfg(feature = "llvm")]
    return sys::LLVM::new().into();

    #[cfg(feature = "singlepass")]
    return sys::Singlepass::new().into();

    #[cfg(feature = "cranelift")]
    return sys::Cranelift::new().into();

    #[cfg(not(any(feature = "cranelift", feature = "llvm", feature = "singlepass")))]
    return Default::default();
}

pub fn compile_wasm(c: &mut BenchmarkGroup<'_, WallTime>, module: &[u8], name: &str) {
    c.bench_function(name, |b| {
        let engine = get_engine();

        b.iter(|| {
            let store = Store::new(engine.clone());
            assert!(Module::new(&store, module).is_ok());
        })
    });
}

pub fn download_and_compile_small(c: &mut BenchmarkGroup<'_, WallTime>) {
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

    let modules = [
        "counter", "primes", "fib_rec", "fib_iter", "bulk_ops", "matmul",
    ];

    for module in modules {
        let bytes =
            reqwest::blocking::get(format!("{BENCHMARKS_ARTIFACTS_BASE_URL}/{module}.wasm"))
                .unwrap()
                .bytes()
                .unwrap();
        compile_wasm(c, bytes.as_ref(), &format!("exec/{name}/{module}"));
    }
}

pub fn download_and_compile_medium(c: &mut BenchmarkGroup<'_, WallTime>) {
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

    let modules = ["bash", "irb", "argon2"];

    for module in modules {
        let bytes =
            reqwest::blocking::get(format!("{BENCHMARKS_ARTIFACTS_BASE_URL}/{module}.wasm"))
                .unwrap()
                .bytes()
                .unwrap();
        compile_wasm(c, bytes.as_ref(), &format!("exec/{name}/{module}"));
    }
}

pub fn download_and_compile_large(c: &mut BenchmarkGroup<'_, WallTime>) {
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

    let modules = [
        "winterjs",
        "wasix_axum",
        "static_web_server",
        "s3_server",
        "python",
        "php",
    ];

    for module in modules {
        let bytes =
            reqwest::blocking::get(format!("{BENCHMARKS_ARTIFACTS_BASE_URL}/{module}.wasm"))
                .unwrap()
                .bytes()
                .unwrap();
        compile_wasm(c, bytes.as_ref(), &format!("exec/{name}/{module}"));
    }
}

pub fn download_and_compile(c: &mut Criterion) {
    {
        let mut group = c.benchmark_group("large_benches");
        group.sample_size(10);
        download_and_compile_large(&mut group);
    }

    {
        let mut group = c.benchmark_group("medium_benches");
        group.sample_size(40);
        download_and_compile_medium(&mut group);
    }
    {
        let mut group = c.benchmark_group("small_benches");
        group.sample_size(60);
        download_and_compile_small(&mut group);
    }
}

criterion_group!(benches, download_and_compile);
criterion_main!(benches);
