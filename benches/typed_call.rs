//! Host-to-Wasm call latency and throughput for small Singlepass exports.
//!
//! Run from the wasmer root:
//!
//! ```shell
//! cargo bench --bench typed_call --features "singlepass experimental-async"
//! ```
//!
//! Compare cached typed and dynamic exports with a non-inlined native add.
//! Async cases time creation and polling of a single call. The plain add
//! completes on its first poll. The suspending add calls a host import,
//! yields once and completes on its second poll. All cases include consuming
//! the result and exclude module compilation and export lookup.

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use wasmer::{Instance, Module, Store, Value, imports, wat2wasm};
use wasmer_compiler_singlepass::Singlepass;

const WAT: &str = r#"
(module
    (func (export "lit") (result i32)
        i32.const 1
        i32.const 1
        i32.add)
    (func (export "add") (param i32 i32) (result i32)
        local.get 0
        local.get 1
        i32.add))
"#;

fn fresh_singlepass_instance() -> (Store, Instance) {
    let wasm = wat2wasm(WAT.as_bytes()).expect("wat parse");
    let compiler = Singlepass::default();
    let mut store = Store::new(compiler);
    let module = Module::new(&store, wasm).expect("singlepass compile");
    let instance = Instance::new(&mut store, &module, &imports! {}).expect("instantiate");
    (store, instance)
}

/// Measures typed call overhead with no arguments.
fn bench_typed_call_lit(c: &mut Criterion) {
    let (mut store, instance) = fresh_singlepass_instance();
    let lit = instance
        .exports
        .get_typed_function::<(), i32>(&store, "lit")
        .expect("export `lit`");

    c.bench_function("typed_call/lit_one_plus_one", |b| {
        b.iter(|| {
            let r = lit.call(&mut store).unwrap();
            black_box(r);
        });
    });
}

#[inline(never)]
fn native_add(a: i32, b: i32) -> i32 {
    a.wrapping_add(b)
}

#[cfg(feature = "experimental-async")]
fn finish_call<F: Future>(future: F, suspends: bool) -> F::Output {
    use std::task::{Context, Poll, Waker};

    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    if suspends {
        assert!(future.as_mut().poll(&mut context).is_pending());
    }
    match future.poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("expected add to complete"),
    }
}

#[cfg(feature = "experimental-async")]
fn suspending_add(store: &mut Store) -> Instance {
    use std::task::Poll;

    let module = Module::new(
        &*store,
        r#"(module
        (import "env" "yield_once" (func $yield_once))
        (func (export "add") (param i32 i32) (result i32)
            call $yield_once
            local.get 0 local.get 1 i32.add))"#,
    )
    .unwrap();
    let yield_once = wasmer::Function::new_typed_async(store, async || {
        let mut yielded = false;
        std::future::poll_fn(|cx| {
            if yielded {
                Poll::Ready(())
            } else {
                yielded = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        })
        .await
    });
    Instance::new(
        store,
        &module,
        &imports! { "env" => { "yield_once" => yield_once } },
    )
    .unwrap()
}

fn bench_typed_call_add(c: &mut Criterion) {
    let (mut store, instance) = fresh_singlepass_instance();
    let add = instance
        .exports
        .get_typed_function::<(i32, i32), i32>(&store, "add")
        .expect("export `add`");

    c.bench_function("typed_call/add_one_plus_one", |b| {
        b.iter(|| {
            let r = add.call(&mut store, black_box(1), black_box(1)).unwrap();
            black_box(r);
        });
    });

    let dynamic = instance.exports.get_function("add").expect("export `add`");
    let mut group = c.benchmark_group("typed_call");
    group.throughput(Throughput::Elements(1));

    group.bench_function("native_add", |b| {
        b.iter(|| black_box(native_add(black_box(1), black_box(1))));
    });

    group.bench_function("dynamic_add", |b| {
        b.iter(|| {
            let args = [Value::I32(black_box(1)), Value::I32(black_box(1))];
            black_box(dynamic.call(&mut store, &args).unwrap());
        });
    });

    #[cfg(feature = "experimental-async")]
    {
        let suspending = suspending_add(&mut store);
        let typed_suspending = suspending
            .exports
            .get_typed_function::<(i32, i32), i32>(&store, "add")
            .unwrap();
        let dynamic_suspending = suspending.exports.get_function("add").unwrap();
        let store = store.into_async();
        for (suffix, typed, function, suspends) in [
            ("", &add, dynamic, false),
            ("_suspend_once", &typed_suspending, dynamic_suspending, true),
        ] {
            group.bench_function(format!("typed_async_add{suffix}"), |b| {
                b.iter(|| {
                    let call = typed.call_async(&store, black_box(1), black_box(1));
                    black_box(finish_call(call, suspends).unwrap());
                });
            });
            group.bench_function(format!("dynamic_async_add{suffix}"), |b| {
                b.iter(|| {
                    let args = vec![Value::I32(black_box(1)), Value::I32(black_box(1))];
                    black_box(finish_call(function.call_async(&store, args), suspends).unwrap());
                });
            });
        }
    }
}

/// Native arithmetic baseline with the add inlined into the timed loop.
fn bench_native_baseline(c: &mut Criterion) {
    c.bench_function("native_baseline/one_plus_one", |b| {
        b.iter(|| {
            let r = black_box(1i32) + black_box(1i32);
            black_box(r);
        });
    });
}

criterion_group!(
    benches,
    bench_typed_call_lit,
    bench_typed_call_add,
    bench_native_baseline,
);
criterion_main!(benches);
