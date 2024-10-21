use criterion::{criterion_group, criterion_main, Criterion};

use wasmer::*;

static BENCHMARKS_ARTIFACTS_BASE_URL: &str = "https://pub-53a226d993e144159d6f8b993fe0cbf3.r2.dev";

pub fn serialize_deserialize(c: &mut Criterion, small: &[u8], medium: &[u8], large: &[u8]) {
    let engine = Engine::default();

    c.bench_function(&format!("rkyv_from-binary_small"), |b| {
        b.iter(|| {
            let module = Module::from_binary(&engine, small);
            assert!(module.is_ok());
            drop(module);
        })
    });

    c.bench_function(&format!("rkyv_from-binary_medium"), |b| {
        b.iter(|| {
            let module = Module::from_binary(&engine, medium);
            assert!(module.is_ok());
            drop(module);
        })
    });

    c.bench_function(&format!("rkyv_from-binary_large"), |b| {
        b.iter(|| {
            let module = Module::from_binary(&engine, large);
            assert!(module.is_ok());
            drop(module);
        })
    });
}

pub fn serialize_deserialize_unchecked(
    c: &mut Criterion,
    small: &[u8],
    medium: &[u8],
    large: &[u8],
) {
    let engine = Engine::default();

    c.bench_function(&format!("rkyv_from-binary-unchecked_small"), |b| {
        b.iter(|| unsafe {
            let module = Module::from_binary_unchecked(&engine, small);
            assert!(module.is_ok());
            drop(module);
        })
    });

    c.bench_function(&format!("rkyv_from-binary-unchecked_medium"), |b| {
        b.iter(|| unsafe {
            let module = Module::from_binary_unchecked(&engine, medium);
            assert!(module.is_ok());
            drop(module);
        })
    });

    c.bench_function(&format!("rkyv_from-binary-unchecked_large"), |b| {
        b.iter(|| unsafe {
            let module = Module::from_binary_unchecked(&engine, large);
            assert!(module.is_ok());
            drop(module);
        })
    });
}

pub fn download_and_run(c: &mut Criterion) {
    let small = reqwest::blocking::get(BENCHMARKS_ARTIFACTS_BASE_URL.to_owned() + "/small.wasm")
        .unwrap()
        .bytes()
        .unwrap();

    let medium = reqwest::blocking::get(BENCHMARKS_ARTIFACTS_BASE_URL.to_owned() + "/medium.wasm")
        .unwrap()
        .bytes()
        .unwrap();

    let large = reqwest::blocking::get(BENCHMARKS_ARTIFACTS_BASE_URL.to_owned() + "/large.wasm")
        .unwrap()
        .bytes()
        .unwrap();

    let small = small.as_ref();
    let medium = medium.as_ref();
    let large = large.as_ref();
    serialize_deserialize(c, small, medium, large);
    serialize_deserialize_unchecked(c, small, medium, large);
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(60);
    targets = download_and_run
);

criterion_main!(benches);
