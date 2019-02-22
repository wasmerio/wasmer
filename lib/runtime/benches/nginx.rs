#[macro_use]
extern crate criterion;
use criterion::Criterion;
use wasmer_runtime::{
    cache::{Cache, FileSystemCache, WasmHash},
    compile, validate,
};

static NGINX_WASM: &'static [u8] = include_bytes!("../../../examples/nginx/nginx.wasm");

fn compile_module() {
    compile(NGINX_WASM).unwrap();
}

fn load_module(cache: &impl Cache) {
    let wasm_hash = WasmHash::generate(NGINX_WASM);
    cache.load(wasm_hash).unwrap();
}

fn validate_benchmark(c: &mut Criterion) {
    c.bench_function("nginx validate", |b| b.iter(|| validate(NGINX_WASM)));
}

fn compile_benchmark(c: &mut Criterion) {
    c.bench_function("nginx compile", |b| b.iter(|| compile_module()));
}

fn load_benchmark(c: &mut Criterion) {
    c.bench_function("nginx load", |b| {
        let mut cache = unsafe { FileSystemCache::new(".").unwrap() };
        let module = compile(NGINX_WASM).unwrap();
        cache.store(module).unwrap();

        b.iter(|| load_module(&cache))
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = validate_benchmark, compile_benchmark, load_benchmark
}
criterion_main!(benches);
