#[macro_use]
extern crate criterion;
use criterion::Criterion;
use tempfile::tempdir;
use wasmer_runtime::{
    cache::{Cache, FileSystemCache, WasmHash},
    compile, validate,
};

static NGINX_WASM: &'static [u8] = include_bytes!("../../../examples/nginx/nginx.wasm");

fn compile_module() {
    compile(NGINX_WASM).unwrap();
}

fn load_module(hash: WasmHash, cache: &impl Cache) {
    cache.load(hash).expect("could not load module");
}

fn hashing_benchmark(c: &mut Criterion) {
    c.bench_function("nginx HASH", |b| b.iter(|| WasmHash::generate(NGINX_WASM)));
}

fn validate_benchmark(c: &mut Criterion) {
    c.bench_function("nginx validate", |b| b.iter(|| validate(NGINX_WASM)));
}

fn compile_benchmark(c: &mut Criterion) {
    c.bench_function("nginx compile", |b| b.iter(|| compile_module()));
}

fn load_benchmark(c: &mut Criterion) {
    c.bench_function("nginx load", |b| {
        let tempdir = tempdir().unwrap();
        let mut cache = unsafe {
            FileSystemCache::new(tempdir.path()).expect("unable to create file system cache")
        };
        let module = compile(NGINX_WASM).unwrap();
        let wasm_hash = WasmHash::generate(NGINX_WASM);
        cache
            .store(wasm_hash, module)
            .expect("unable to store into cache");

        b.iter(|| load_module(wasm_hash, &cache))
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = validate_benchmark, hashing_benchmark, compile_benchmark, load_benchmark
}
criterion_main!(benches);
