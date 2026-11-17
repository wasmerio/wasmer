//! Cached direct, nested, and asynchronous Rust-to-Wasm export calls.
use criterion::{Criterion, criterion_group, criterion_main};
use std::{
    hint::black_box,
    task::{Context, Poll, Waker},
};
use wasmer::{Function, Instance, Module, Store, Value, imports};

const ADD: &str = r#"(module (func (export "add") (param i32 i32) (result i32)
    local.get 0 local.get 1 i32.add))"#;
#[cfg(feature = "experimental-async")]
const SUSPEND: &str = r#"(module (import "host" "wait" (func $wait))
  (func (export "add") (param i32 i32) (result i32)
    call $wait local.get 0 local.get 1 i32.add))"#;

#[inline(never)]
fn native_add(a: i32, b: i32) -> i32 {
    a.wrapping_add(b)
}

#[cfg(feature = "experimental-async")]
fn finish<F: Future>(f: F, pending_once: bool) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = Context::from_waker(Waker::noop());
    if pending_once {
        assert!(f.as_mut().poll(&mut cx).is_pending());
    }
    match f.poll(&mut cx) {
        Poll::Ready(x) => x,
        Poll::Pending => panic!("unexpected suspension"),
    }
}

fn wasmer_cases(c: &mut Criterion, mut store: Store, bytes: &[u8]) {
    let module = Module::new(&store, bytes).unwrap();
    let instance = Instance::new(&mut store, &module, &imports! {}).unwrap();
    let typed = instance
        .exports
        .get_typed_function::<(i32, i32), i32>(&store, "add")
        .unwrap();
    let dynamic = instance.exports.get_function("add").unwrap();
    assert_eq!(typed.call(&mut store, 15, 27).unwrap(), 42);
    assert_eq!(
        dynamic
            .call(&mut store, &[Value::I32(15), Value::I32(27)])
            .unwrap()[0],
        Value::I32(42)
    );
    c.bench_function("llvm/typed", |b| {
        b.iter(|| {
            black_box(
                typed
                    .call(&mut store, black_box(15), black_box(27))
                    .unwrap(),
            )
        })
    });

    c.bench_function("llvm/dynamic", |b| {
        b.iter(|| {
            black_box(
                dynamic
                    .call(
                        &mut store,
                        &[Value::I32(black_box(15)), Value::I32(black_box(27))],
                    )
                    .unwrap()[0]
                    .unwrap_i32(),
            )
        })
    });
    if std::env::var_os("CALL_STUDY_VERIFY").is_some() {
        wasmer_reentry::<true>(c, &mut store, &instance)
    } else {
        wasmer_reentry::<false>(c, &mut store, &instance)
    };
    #[cfg(feature = "experimental-async")]
    {
        let module = Module::new(&store, SUSPEND).unwrap();
        let wait = Function::new_typed_async(&mut store, suspend_once);
        let suspended =
            Instance::new(&mut store, &module, &imports! {"host" => {"wait" => wait}}).unwrap();
        let susp_typed = suspended
            .exports
            .get_typed_function::<(i32, i32), i32>(&store, "add")
            .unwrap();
        let susp_dyn = suspended.exports.get_function("add").unwrap();
        let store = store.into_async();
        for (kind, t, d, pending) in [
            ("ready", &typed, dynamic, false),
            ("suspend", &susp_typed, susp_dyn, true),
        ] {
            assert_eq!(finish(t.call_async(&store, 15, 27), pending).unwrap(), 42);
            assert_eq!(
                finish(
                    d.call_async(&store, vec![Value::I32(15), Value::I32(27)]),
                    pending
                )
                .unwrap()[0],
                Value::I32(42)
            );
            c.bench_function(&format!("llvm/typed_{kind}"), |b| {
                b.iter(|| {
                    black_box(
                        finish(t.call_async(&store, black_box(15), black_box(27)), pending)
                            .unwrap(),
                    )
                })
            });
            c.bench_function(&format!("llvm/dynamic_{kind}"), |b| {
                b.iter(|| {
                    black_box(
                        finish(
                            d.call_async(
                                &store,
                                vec![Value::I32(black_box(15)), Value::I32(black_box(27))],
                            ),
                            pending,
                        )
                        .unwrap()[0]
                            .unwrap_i32(),
                    )
                })
            });
        }
    }
}

async fn suspend_once() {
    let mut yielded = false;
    std::future::poll_fn(|cx| {
        if yielded {
            return Poll::Ready(());
        }
        yielded = true;
        cx.waker().wake_by_ref();
        Poll::Pending
    })
    .await
}

fn wasmtime_cases(c: &mut Criterion, bytes: &[u8]) {
    use wasmtime::{Config, Engine, Instance, Linker, Module, Store, Strategy, Val};
    let mut cfg = Config::new();
    cfg.strategy(Strategy::Cranelift);
    let engine = Engine::new(&cfg).unwrap();
    let module = Module::new(&engine, bytes).unwrap();
    let mut store = Store::new(&engine, ());
    let ready = Instance::new(&mut store, &module, &[]).unwrap();
    let typed = ready
        .get_typed_func::<(i32, i32), i32>(&mut store, "add")
        .unwrap();
    let dynamic = ready.get_func(&mut store, "add").unwrap();
    let mut output = [Val::I32(0)];
    assert_eq!(typed.call(&mut store, (15, 27)).unwrap(), 42);
    dynamic
        .call(&mut store, &[Val::I32(15), Val::I32(27)], &mut output)
        .unwrap();
    assert_eq!(output[0].i32(), Some(42));
    c.bench_function("wasmtime/typed", |b| {
        b.iter(|| {
            black_box(
                typed
                    .call(&mut store, (black_box(15), black_box(27)))
                    .unwrap(),
            )
        })
    });
    c.bench_function("wasmtime/dynamic", |b| {
        b.iter(|| {
            dynamic
                .call(
                    &mut store,
                    &[Val::I32(black_box(15)), Val::I32(black_box(27))],
                    &mut output,
                )
                .unwrap();
            black_box(output[0].i32().unwrap())
        })
    });

    if std::env::var_os("CALL_STUDY_VERIFY").is_some() {
        wasmtime_reentry::<true>(c, &mut store, &ready)
    } else {
        wasmtime_reentry::<false>(c, &mut store, &ready)
    };
    let suspend = Module::new(&engine, wasmer::wat2wasm(SUSPEND.as_bytes()).unwrap()).unwrap();
    let mut linker = Linker::new(&engine);
    linker
        .func_wrap_async("host", "wait", |_, ()| Box::new(suspend_once()))
        .unwrap();
    let suspended = finish(linker.instantiate_async(&mut store, &suspend), false).unwrap();
    let suspend_typed = suspended
        .get_typed_func::<(i32, i32), i32>(&mut store, "add")
        .unwrap();
    let suspend_dynamic = suspended.get_func(&mut store, "add").unwrap();
    for (kind, t, d, pending) in [
        ("ready", &typed, dynamic, false),
        ("suspend", &suspend_typed, suspend_dynamic, true),
    ] {
        assert_eq!(
            finish(t.call_async(&mut store, (15, 27)), pending).unwrap(),
            42
        );
        finish(
            d.call_async(&mut store, &[Val::I32(15), Val::I32(27)], &mut output),
            pending,
        )
        .unwrap();
        assert_eq!(output[0].i32(), Some(42));
        c.bench_function(&format!("wasmtime/typed_{kind}"), |b| {
            b.iter(|| {
                black_box(
                    finish(
                        t.call_async(&mut store, (black_box(15), black_box(27))),
                        pending,
                    )
                    .unwrap(),
                )
            })
        });
        c.bench_function(&format!("wasmtime/dynamic_{kind}"), |b| {
            b.iter(|| {
                finish(
                    d.call_async(
                        &mut store,
                        &[Val::I32(black_box(15)), Val::I32(black_box(27))],
                        &mut output,
                    ),
                    pending,
                )
                .unwrap();
                black_box(output[0].i32().unwrap())
            })
        });
    }
}

// Each wrapper executes real guest code both before and after the host import.
// One application of WRAP adds two to the leaf result, so depth is observable.
const WRAP: &str = r#"(module (import "h" "call" (func $call (param i32 i32) (result i32)))
    (func (export "run") (param i32 i32) (result i32)
        local.get 0 i32.const 1 i32.add local.get 1 call $call i32.const 1 i32.add))"#;

thread_local! { static HOST_ENTRIES: std::cell::Cell<i32> = const { std::cell::Cell::new(0) }; }
#[inline]
fn record_host<const VERIFY: bool>() {
    if VERIFY {
        HOST_ENTRIES.with(|n| n.set(n.get() + 1));
    }
}

const COUNTED_ADD: &str = r#"(module
    (global $hits (export "hits") (mut i32) (i32.const 0))
    (func (export "add") (param i32 i32) (result i32)
      global.get $hits i32.const 1 i32.add global.set $hits
      local.get 0 local.get 1 i32.add))"#;

type WTyped = wasmer::TypedFunction<(i32, i32), i32>;
type TTyped = wasmtime::TypedFunc<(i32, i32), i32>;

fn wasmer_reentry<const VERIFY: bool>(c: &mut Criterion, store: &mut Store, leaf: &Instance) {
    use wasmer::{FunctionEnv, FunctionEnvMut};
    let module = Module::new(&*store, WRAP).unwrap();
    let import_only = Function::new_typed(store, native_add);
    let wrapper = |store: &mut Store, f: Function| {
        Instance::new(store, &module, &imports! { "h" => {"call" => f} }).unwrap()
    };
    let just_host = wrapper(store, import_only);
    let imports = just_host
        .exports
        .get_typed_function::<(i32, i32), i32>(&*store, "run")
        .unwrap();
    assert_eq!(imports.call(store, 15, 27).unwrap(), 44);
    c.bench_function("llvm/import_only", |b| {
        b.iter(|| {
            black_box(
                imports
                    .call(&mut *store, black_box(15), black_box(27))
                    .unwrap(),
            )
        })
    });
    let mut typed = leaf
        .exports
        .get_typed_function::<(i32, i32), i32>(&*store, "add")
        .unwrap();
    let mut dynamic = leaf.exports.get_function("add").unwrap().clone();
    for depth in 1..=2 {
        let typed_env = FunctionEnv::new(&mut *store, typed);
        let cb = Function::new_typed_with_env(
            &mut *store,
            &typed_env,
            |mut e: FunctionEnvMut<WTyped>, a: i32, b: i32| {
                record_host::<VERIFY>();
                let (inner, mut store) = e.data_and_store_mut();
                inner.call(&mut store, a, b).unwrap()
            },
        );
        let typed_instance = wrapper(store, cb);
        typed = typed_instance
            .exports
            .get_typed_function::<(i32, i32), i32>(&*store, "run")
            .unwrap();
        let dyn_env = FunctionEnv::new(&mut *store, dynamic);
        let cb = Function::new_typed_with_env(
            &mut *store,
            &dyn_env,
            |mut e: FunctionEnvMut<Function>, a: i32, b: i32| {
                record_host::<VERIFY>();
                let (inner, mut store) = e.data_and_store_mut();
                inner
                    .call(&mut store, &[Value::I32(a), Value::I32(b)])
                    .unwrap()[0]
                    .unwrap_i32()
            },
        );
        let dyn_instance = wrapper(store, cb);
        dynamic = dyn_instance.exports.get_function("run").unwrap().clone();
        let hits = |s: &mut Store| {
            leaf.exports
                .get_global("hits")
                .unwrap()
                .get(&mut *s)
                .unwrap_i32()
        };
        let before = if VERIFY {
            HOST_ENTRIES.with(|n| n.set(0));
            hits(store)
        } else {
            0
        };
        assert_eq!(typed.call(store, 15, 27).unwrap(), 42 + 2 * depth);
        if VERIFY {
            assert_eq!(hits(store) - before, 1);
            HOST_ENTRIES.with(|n| assert_eq!(n.get(), depth));
        }

        assert_eq!(
            dynamic.call(store, &[15.into(), 27.into()]).unwrap()[0].unwrap_i32(),
            42 + 2 * depth
        );
        if VERIFY {
            assert_eq!(hits(store) - before, 2);
            HOST_ENTRIES.with(|n| assert_eq!(n.get(), 2 * depth));
            println!(
                "Verified each depth-{depth} chain enters {depth} Rust imports and one Wasm add"
            );
        }
        c.bench_function(&format!("llvm/nested_typed_{depth}"), |b| {
            b.iter(|| {
                black_box(
                    typed
                        .call(&mut *store, black_box(15), black_box(27))
                        .unwrap(),
                )
            })
        });
        c.bench_function(&format!("llvm/nested_dynamic_{depth}"), |b| {
            b.iter(|| {
                black_box(
                    dynamic
                        .call(
                            &mut *store,
                            &[Value::I32(black_box(15)), Value::I32(black_box(27))],
                        )
                        .unwrap()[0]
                        .unwrap_i32(),
                )
            })
        });
    }
}

fn wasmtime_reentry<const VERIFY: bool>(
    c: &mut Criterion,
    store: &mut wasmtime::Store<()>,
    leaf: &wasmtime::Instance,
) {
    use wasmtime::{Caller, Func, Instance, Val};
    let module =
        wasmtime::Module::new(store.engine(), wasmer::wat2wasm(WRAP.as_bytes()).unwrap()).unwrap();
    let wrapper =
        |s: &mut wasmtime::Store<()>, f: Func| Instance::new(s, &module, &[f.into()]).unwrap();
    let cb = Func::wrap(&mut *store, native_add);
    let inst = wrapper(store, cb);
    let import_only: TTyped = inst.get_typed_func(&mut *store, "run").unwrap();
    assert_eq!(import_only.call(&mut *store, (15, 27)).unwrap(), 44);
    c.bench_function("wasmtime/import_only", |b| {
        b.iter(|| {
            black_box(
                import_only
                    .call(&mut *store, (black_box(15), black_box(27)))
                    .unwrap(),
            )
        })
    });
    let mut typed: TTyped = leaf.get_typed_func(&mut *store, "add").unwrap();
    let mut dynamic = leaf.get_func(&mut *store, "add").unwrap();
    for depth in 1..=2 {
        let cb = Func::wrap(
            &mut *store,
            move |caller: Caller<'_, ()>, a: i32, b: i32| {
                record_host::<VERIFY>();
                typed.call(caller, (a, b)).unwrap()
            },
        );
        let inst = wrapper(store, cb);
        typed = inst.get_typed_func(&mut *store, "run").unwrap();
        let cb = Func::wrap(
            &mut *store,
            move |caller: Caller<'_, ()>, a: i32, b: i32| {
                record_host::<VERIFY>();
                let mut slot = [Val::I32(0)];
                dynamic
                    .call(caller, &[Val::I32(a), Val::I32(b)], &mut slot)
                    .unwrap();
                slot[0].i32().unwrap()
            },
        );
        let inst = wrapper(store, cb);
        dynamic = inst.get_func(&mut *store, "run").unwrap();
        let mut slot = [Val::I32(0)];
        let hits = |s: &mut wasmtime::Store<()>| {
            leaf.get_global(&mut *s, "hits")
                .unwrap()
                .get(&mut *s)
                .i32()
                .unwrap()
        };
        let before = if VERIFY {
            HOST_ENTRIES.with(|n| n.set(0));
            hits(store)
        } else {
            0
        };
        assert_eq!(typed.call(&mut *store, (15, 27)).unwrap(), 42 + 2 * depth);
        if VERIFY {
            assert_eq!(hits(store) - before, 1);
            HOST_ENTRIES.with(|n| assert_eq!(n.get(), depth));
        }

        dynamic
            .call(&mut *store, &[Val::I32(15), Val::I32(27)], &mut slot)
            .unwrap();
        assert_eq!(slot[0].i32().unwrap(), 42 + 2 * depth);
        if VERIFY {
            assert_eq!(hits(store) - before, 2);
            HOST_ENTRIES.with(|n| assert_eq!(n.get(), 2 * depth));
            println!(
                "Verified each depth-{depth} chain enters {depth} Rust imports and one Wasm add"
            );
        }
        c.bench_function(&format!("wasmtime/nested_typed_{depth}"), |b| {
            b.iter(|| {
                black_box(
                    typed
                        .call(&mut *store, (black_box(15), black_box(27)))
                        .unwrap(),
                )
            })
        });
        c.bench_function(&format!("wasmtime/nested_dynamic_{depth}"), |b| {
            b.iter(|| {
                dynamic
                    .call(
                        &mut *store,
                        &[Val::I32(black_box(15)), Val::I32(black_box(27))],
                        &mut slot,
                    )
                    .unwrap();
                black_box(slot[0].i32().unwrap())
            })
        });
    }
}

fn run(c: &mut Criterion) {
    let wat = if std::env::var_os("CALL_STUDY_VERIFY").is_some() {
        COUNTED_ADD
    } else {
        ADD
    };
    let bytes = wasmer::wat2wasm(wat.as_bytes()).unwrap();
    wasmer_cases(c, Store::new(wasmer_compiler_llvm::LLVM::new()), &bytes);
    wasmtime_cases(c, &bytes);
    c.bench_function("control/native", |b| {
        b.iter(|| black_box(native_add(black_box(15), black_box(27))))
    });
}
criterion_group!(benches, run);
criterion_main!(benches);
