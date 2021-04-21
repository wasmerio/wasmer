use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use tempfile::TempDir;
use wasmer::{Module, Store};
use wasmer_cache::Cache;
use wasmer_cache::{FileSystemCache, Hash};
use wasmer_compiler_singlepass::Singlepass;
use wasmer_engine_jit::JIT;
use wasmer_engine_native::Native;

fn random_key() -> Hash {
    Hash::new(rand::thread_rng().gen::<[u8; 32]>())
}

pub fn store_cache_jit(c: &mut Criterion) {
    let tmp_dir = TempDir::new().unwrap();
    let mut fs_cache = FileSystemCache::new(tmp_dir.path()).unwrap();
    let compiler = Singlepass::default();
    let store = Store::new(&JIT::new(compiler).engine());
    let module = Module::new(
        &store,
        std::fs::read("../../lib/c-api/tests/assets/qjs.wasm").unwrap(),
    )
    .unwrap();

    c.bench_function("store jit module in filesystem cache", |b| {
        b.iter(|| {
            let key = random_key();
            fs_cache.store(key, &module).unwrap()
        })
    });
}

pub fn load_cache_jit(c: &mut Criterion) {
    let tmp_dir = TempDir::new().unwrap();
    let mut fs_cache = FileSystemCache::new(tmp_dir.path()).unwrap();
    let compiler = Singlepass::default();
    let store = Store::new(&JIT::new(compiler).engine());
    let module = Module::new(
        &store,
        std::fs::read("../../lib/c-api/tests/assets/qjs.wasm").unwrap(),
    )
    .unwrap();
    let key = Hash::new([0u8; 32]);
    fs_cache.store(key, &module).unwrap();

    c.bench_function("load jit module in filesystem cache", |b| {
        b.iter(|| unsafe { fs_cache.load(&store, key.clone()).unwrap() })
    });
}

pub fn store_cache_native(c: &mut Criterion) {
    let tmp_dir = TempDir::new().unwrap();
    let mut fs_cache = FileSystemCache::new(tmp_dir.path()).unwrap();
    let compiler = Singlepass::default();
    let store = Store::new(&Native::new(compiler).engine());
    let module = Module::new(
        &store,
        std::fs::read("../../lib/c-api/tests/assets/qjs.wasm").unwrap(),
    )
    .unwrap();

    c.bench_function("store native module in filesystem cache", |b| {
        b.iter(|| {
            let key = random_key();
            fs_cache.store(key, &module).unwrap()
        })
    });
}

pub fn load_cache_native(c: &mut Criterion) {
    let tmp_dir = TempDir::new().unwrap();
    let mut fs_cache = FileSystemCache::new(tmp_dir.path()).unwrap();
    let compiler = Singlepass::default();
    let store = Store::new(&Native::new(compiler).engine());
    let module = Module::new(
        &store,
        std::fs::read("../../lib/c-api/tests/assets/qjs.wasm").unwrap(),
    )
    .unwrap();
    let key = Hash::new([0u8; 32]);
    fs_cache.store(key, &module).unwrap();

    c.bench_function("load native module in filesystem cache", |b| {
        b.iter(|| unsafe { fs_cache.load(&store, key.clone()).unwrap() })
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(300);
    targets = store_cache_jit, load_cache_jit, store_cache_native, load_cache_native
}
criterion_main!(benches);
