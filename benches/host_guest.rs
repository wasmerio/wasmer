use std::hint::black_box;

use criterion::{
    BenchmarkGroup, Criterion, Throughput, criterion_group, criterion_main, measurement::WallTime,
};
use wasmer::{Function, FunctionEnv, FunctionEnvMut, Instance, Module, Store, imports};

const HOST_CALLS_PER_INVOCATION: u32 = 10_000;

fn module_wasm() -> Vec<u8> {
    let wat = format!(
        r#"(module
            (import "host" "hello" (func $host_hello
                (param i32 i32 i32 i32 i32 i32 i32 i32)
                (result i32)))
            (func $hello_loop (local $remaining i32)
                (local.set $remaining (i32.const {HOST_CALLS_PER_INVOCATION}))
                (loop $repeat
                    (drop
                        (call $host_hello
                            (i32.const 13)
                            (i32.const 87)
                            (i32.const 41)
                            (i32.const 92)
                            (i32.const 56)
                            (i32.const 74)
                            (i32.const 18)
                            (i32.const 65)))
                    (local.set $remaining (i32.sub (local.get $remaining) (i32.const 1)))
                    (br_if $repeat (i32.gt_s (local.get $remaining) (i32.const 0)))))
            (func (export "hello")
                (call $hello_loop)))"#
    );
    wasmer::wat2wasm(wat.as_bytes()).unwrap().into_owned()
}

fn bench_wasmer(store: &mut Store, wasm: &[u8], group: &mut BenchmarkGroup<'_, WallTime>) {
    let module = Module::new(store, wasm).unwrap();
    let env = FunctionEnv::new(store, 0_u32);
    let import_object = imports! {
        "host" => {
            "hello" => Function::new_typed_with_env(
                store,
                &env,
                |mut env: FunctionEnvMut<'_, u32>,
                 _a1: i32,
                 _a2: i32,
                 _a3: i32,
                 _a4: i32,
                 a5: i32,
                 _a6: i32,
                 _a7: i32,
                 _a8: i32|
                 -> i32 {
                    *env.data_mut() += 1;
                    a5
                },
            ),
        }
    };
    let instance = Instance::new(store, &module, &import_object).unwrap();
    let hello = instance
        .exports
        .get_typed_function::<(), ()>(store, "hello")
        .unwrap();

    hello.call(store).unwrap();
    assert_eq!(*env.as_ref(store), HOST_CALLS_PER_INVOCATION);

    group.bench_function(format!("with_env/wasmer/llvm"), |b| {
        b.iter(|| {
            *env.as_mut(store) = 0;
            hello.call(store).unwrap();
            black_box(*env.as_ref(store));
        });
    });
}

fn bench_wasmtime(wasm: &[u8], group: &mut BenchmarkGroup<'_, WallTime>) {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm).unwrap();
    let mut store = wasmtime::Store::new(&engine, 0_u32);
    let mut linker = wasmtime::Linker::new(&engine);
    linker
        .func_wrap(
            "host",
            "hello",
            |mut caller: wasmtime::Caller<'_, u32>,
             _a1: i32,
             _a2: i32,
             _a3: i32,
             _a4: i32,
             a5: i32,
             _a6: i32,
             _a7: i32,
             _a8: i32|
             -> i32 {
                *caller.data_mut() += 1;
                a5
            },
        )
        .unwrap();
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let hello = instance
        .get_typed_func::<(), ()>(&mut store, "hello")
        .unwrap();

    hello.call(&mut store, ()).unwrap();
    assert_eq!(*store.data(), HOST_CALLS_PER_INVOCATION);

    group.bench_function("with_env/wasmtime", |b| {
        b.iter(|| {
            *store.data_mut() = 0;
            hello.call(&mut store, ()).unwrap();
            black_box(*store.data());
        });
    });
}

fn bench_wasmer_without_env(
    store: &mut Store,
    wasm: &[u8],
    group: &mut BenchmarkGroup<'_, WallTime>,
) {
    let module = Module::new(store, wasm).unwrap();
    let import_object = imports! {
        "host" => {
            "hello" => Function::new_typed(
                store,
                |_a1: i32,
                 _a2: i32,
                 _a3: i32,
                 _a4: i32,
                 a5: i32,
                 _a6: i32,
                 _a7: i32,
                 _a8: i32|
                 -> i32 { a5 },
            ),
        }
    };
    let instance = Instance::new(store, &module, &import_object).unwrap();
    let hello = instance
        .exports
        .get_typed_function::<(), ()>(store, "hello")
        .unwrap();

    hello.call(store).unwrap();

    group.bench_function("without_env/wasmer/llvm", |b| {
        b.iter(|| hello.call(store).unwrap());
    });
}

fn bench_wasmtime_without_env(wasm: &[u8], group: &mut BenchmarkGroup<'_, WallTime>) {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let mut linker = wasmtime::Linker::new(&engine);
    linker
        .func_wrap(
            "host",
            "hello",
            |_a1: i32,
             _a2: i32,
             _a3: i32,
             _a4: i32,
             a5: i32,
             _a6: i32,
             _a7: i32,
             _a8: i32|
             -> i32 { a5 },
        )
        .unwrap();
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let hello = instance
        .get_typed_func::<(), ()>(&mut store, "hello")
        .unwrap();

    hello.call(&mut store, ()).unwrap();

    group.bench_function("without_env/wasmtime", |b| {
        b.iter(|| hello.call(&mut store, ()).unwrap());
    });
}

fn run_benchmarks(c: &mut Criterion) {
    let wasm = module_wasm();
    let mut group = c.benchmark_group("host_guest");
    group.throughput(Throughput::Elements(u64::from(HOST_CALLS_PER_INVOCATION)));

    bench_wasmer(
        &mut Store::new(wasmer_compiler_llvm::LLVM::new()),
        &wasm,
        &mut group,
    );

    bench_wasmtime(&wasm, &mut group);

    bench_wasmer_without_env(
        &mut Store::new(wasmer_compiler_llvm::LLVM::new()),
        &wasm,
        &mut group,
    );
    bench_wasmtime_without_env(&wasm, &mut group);
    group.finish();
}

criterion_group!(benches, run_benchmarks);
criterion_main!(benches);
