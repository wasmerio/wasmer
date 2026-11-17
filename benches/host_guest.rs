use std::hint::black_box;

use criterion::{
    BenchmarkGroup, Criterion, Throughput, criterion_group, criterion_main, measurement::WallTime,
};
use wasmer::{
    Function, FunctionEnv, FunctionEnvMut, FunctionType, Instance, Module, Store, Type, Value,
    imports,
};

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

    group.bench_function("with_env/wasmer/llvm", |b| {
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

fn identity_module_wasm() -> Vec<u8> {
    wasmer::wat2wasm(
        br#"(module
            (import "wasmbench" "identity"
                (func $identity (param i32) (result i32)))
            (func (export "benchmark")
                (param $value i32)
                (param $iterations i32)
                (result i32)
                (block $done
                    local.get $iterations
                    i32.eqz
                    br_if $done
                    (loop $repeat
                        local.get $value
                        call $identity
                        local.set $value
                        local.get $iterations
                        i32.const 1
                        i32.sub
                        local.tee $iterations
                        br_if $repeat))
                local.get $value))"#,
    )
    .unwrap()
    .into_owned()
}

fn bench_wasmer_identity(store: &mut Store, wasm: &[u8], group: &mut BenchmarkGroup<'_, WallTime>) {
    let module = Module::new(store, wasm).unwrap();
    let import_object = imports! {
        "wasmbench" => {
            "identity" => Function::new_typed(store, |value: i32| -> i32 { value }),
        }
    };
    let instance = Instance::new(store, &module, &import_object).unwrap();
    let benchmark = instance
        .exports
        .get_typed_function::<(i32, i32), i32>(store, "benchmark")
        .unwrap();

    assert_eq!(benchmark.call(store, 7, 0).unwrap(), 7);
    assert_eq!(benchmark.call(store, 7, 1_000_000).unwrap(), 7);

    group.bench_function("identity/wasmer/llvm", |b| {
        b.iter(|| black_box(benchmark.call(store, 7, 1_000_000).unwrap()));
    });
}

fn bench_wasmtime_identity(wasm: &[u8], group: &mut BenchmarkGroup<'_, WallTime>) {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let mut linker = wasmtime::Linker::new(&engine);
    linker
        .func_wrap("wasmbench", "identity", |value: i32| -> i32 { value })
        .unwrap();
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let benchmark = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "benchmark")
        .unwrap();

    assert_eq!(benchmark.call(&mut store, (7, 0)).unwrap(), 7);
    assert_eq!(benchmark.call(&mut store, (7, 1_000_000)).unwrap(), 7);

    group.bench_function("identity/wasmtime", |b| {
        b.iter(|| black_box(benchmark.call(&mut store, (7, 1_000_000)).unwrap()));
    });
}

/// One typed entry drives every import in the batch. Each import's return
/// becomes the next import's argument, so the final value checks the whole loop.
fn bench_wasmer_imports(wasm: &[u8], group: &mut BenchmarkGroup<'_, WallTime>) {
    const COUNT: i32 = 4096;
    let mut store = Store::new(wasmer_compiler_llvm::LLVM::new());
    let module = Module::new(&store, wasm).unwrap();
    let signature = FunctionType::new([Type::I32], [Type::I32]);
    let typed = Function::new_typed(&mut store, |v: i32| v + 1);
    let dynamic = Function::new(&mut store, signature.clone(), |args| {
        Ok(vec![Value::I32(args[0].unwrap_i32() + 1)])
    });
    let bind = |store: &mut Store, import: Function| {
        Instance::new(
            store,
            &module,
            &imports! { "wasmbench" => { "identity" => import } },
        )
        .unwrap()
        .exports
        .get_typed_function::<(i32, i32), i32>(store, "benchmark")
        .unwrap()
    };
    group.throughput(Throughput::Elements(COUNT as u64));
    for (name, import) in [("typed", typed), ("dynamic", dynamic)] {
        let run = bind(&mut store, import);
        assert_eq!(run.call(&mut store, 7, COUNT).unwrap(), 7 + COUNT);
        group.bench_function(format!("imports/llvm/{name}"), |b| {
            b.iter(|| black_box(run.call(&mut store, black_box(7), COUNT).unwrap()))
        });
    }

    #[cfg(feature = "experimental-async")]
    {
        let runs = [false, true].map(|suspend| {
            let typed = Function::new_typed_async(&mut store, move |v: i32| step(v, suspend));
            let dynamic = Function::new_async(&mut store, signature.clone(), move |args| {
                let value = args[0].unwrap_i32();
                async move { Ok(vec![Value::I32(step(value, suspend).await)]) }
            });
            [
                ("typed", suspend, bind(&mut store, typed)),
                ("dynamic", suspend, bind(&mut store, dynamic)),
            ]
        });
        let async_store = store.into_async();
        for (name, suspend, run) in runs.into_iter().flatten() {
            let batch = || {
                use std::task::{Context, Poll, Waker};
                let mut future = std::pin::pin!(run.call_async(&async_store, black_box(7), COUNT));
                let mut cx = Context::from_waker(Waker::noop());
                let mut pending = 0;
                loop {
                    match future.as_mut().poll(&mut cx) {
                        Poll::Ready(value) => break (value.unwrap(), pending),
                        Poll::Pending => pending += 1,
                    }
                }
            };
            assert_eq!(batch(), (7 + COUNT, if suspend { COUNT } else { 0 }));
            let mode = if suspend { "suspend_once" } else { "ready" };
            group.bench_function(format!("imports/llvm/{name}_{mode}"), |b| {
                b.iter(|| black_box(batch()))
            });
        }
    }
}

#[cfg(feature = "experimental-async")]
async fn step(value: i32, mut suspend: bool) -> i32 {
    std::future::poll_fn(|cx| {
        if std::mem::take(&mut suspend) {
            cx.waker().wake_by_ref();
            std::task::Poll::Pending
        } else {
            std::task::Poll::Ready(value + 1)
        }
    })
    .await
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

    let identity_wasm = identity_module_wasm();
    group.throughput(Throughput::Elements(1_000_000));
    bench_wasmer_identity(
        &mut Store::new(wasmer_compiler_llvm::LLVM::new()),
        &identity_wasm,
        &mut group,
    );
    bench_wasmtime_identity(&identity_wasm, &mut group);
    bench_wasmer_imports(&identity_wasm, &mut group);
    group.finish();
}

criterion_group!(benches, run_benchmarks);
criterion_main!(benches);
