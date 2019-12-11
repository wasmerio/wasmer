#[macro_use]
extern crate criterion;
use criterion::Criterion;
use tempfile::tempdir;
use wasmer_runtime::{
    cache::{Cache, FileSystemCache, WasmHash},
    compile, func, imports, instantiate, validate,
};
use wasmer_runtime_core::vm::Ctx;

fn it_works(_ctx: &mut Ctx) -> i32 {
    5
}

static SIMPLE_WASM: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/no_abi_simple_plugin.wasm"
));

fn hashing_benchmark(c: &mut Criterion) {
    c.bench_function("HASH", |b| b.iter(|| WasmHash::generate(SIMPLE_WASM)));
}

fn validate_benchmark(c: &mut Criterion) {
    c.bench_function("validate", |b| b.iter(|| validate(SIMPLE_WASM)));
}

fn compile_benchmark(c: &mut Criterion) {
    c.bench_function("compile", |b| b.iter(|| compile(SIMPLE_WASM)));
}

fn create_instance_benchmark(c: &mut Criterion) {
    let imports = imports!(
        "env" => {
            "it_works" => func!(it_works),
        },
    );
    c.bench_function("instantiate", move |b| {
        b.iter(|| instantiate(&SIMPLE_WASM[..], &imports).unwrap())
    });
}

fn create_instance_from_cache_benchmark(c: &mut Criterion) {
    let imports = imports!(
        "env" => {
            "it_works" => func!(it_works),
        },
    );
    let tempdir = tempdir().unwrap();
    let mut cache = unsafe {
        FileSystemCache::new(tempdir.path()).expect("unable to create file system cache")
    };
    let module = compile(SIMPLE_WASM).unwrap();
    let hash = WasmHash::generate(SIMPLE_WASM);
    cache
        .store(hash, module)
        .expect("unable to store into cache");
    c.bench_function("instantiate from cache", move |b| {
        b.iter(|| {
            let module = cache.load(hash).unwrap();
            module.instantiate(&imports).unwrap();
        })
    });
}

fn calling_fn_benchmark(c: &mut Criterion) {
    let imports = imports!(
        "env" => {
            "it_works" => func!(it_works),
        },
    );
    let instance = instantiate(SIMPLE_WASM, &imports).unwrap();
    c.bench_function("calling fn", move |b| {
        let entry_point = instance.func::<i32, i32>("plugin_entrypoint").unwrap();
        b.iter(|| entry_point.call(2).unwrap())
    });
}

criterion_group! {
    name = instance_bench;
    config = Criterion::default().sample_size(20);
    targets = compile_benchmark, validate_benchmark, hashing_benchmark, create_instance_from_cache_benchmark, calling_fn_benchmark, create_instance_benchmark,
}
criterion_main!(instance_bench);
