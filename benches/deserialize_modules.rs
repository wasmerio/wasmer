use criterion::{criterion_group, criterion_main, Criterion};

use wasmer::*;

static BENCHMARKS_ARTIFACTS_BASE_URL: &str = "https://pub-53a226d993e144159d6f8b993fe0cbf3.r2.dev";

pub fn serialize_deserialize(c: &mut Criterion, bytes: &[u8], name: &str) {
    let engine = Engine::default();

    c.bench_function(&format!("rkyv_from-binary_{name}"), |b| {
        b.iter(|| {
            let module = Module::from_binary(&engine, bytes);
            assert!(module.is_ok());
            drop(module);
        })
    });
}

pub fn serialize_deserialize_unchecked(c: &mut Criterion, bytes: &[u8], name: &str) {
    let engine = Engine::default();

    c.bench_function(&format!("rkyv_from-binary-unchecked_{name}"), |b| {
        b.iter(|| unsafe {
            let module = Module::from_binary_unchecked(&engine, bytes);
            assert!(module.is_ok());
            drop(module);
        })
    });
}

pub fn download_and_run(c: &mut Criterion) {
    let modules = ["small", "medium", "large"];

    for module in modules {
        let bytes =
            reqwest::blocking::get(format!("{BENCHMARKS_ARTIFACTS_BASE_URL}/{module}.wasm"))
                .unwrap()
                .bytes()
                .unwrap();
        serialize_deserialize(c, &bytes, module);
        serialize_deserialize_unchecked(c, &bytes, module);
    }
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(60);
    targets = download_and_run
);

criterion_main!(benches);
