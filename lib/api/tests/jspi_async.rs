#![cfg(all(feature = "experimental-async", not(target_arch = "wasm32")))]

use std::{
    cell::RefCell,
    sync::{Arc, Mutex, OnceLock},
};

use anyhow::Result;
use futures::future;
use wasmer::{
    AsyncFunctionEnvMut, Function, FunctionEnv, FunctionEnvMut, FunctionType, Instance, Module,
    Store, StoreAsync, Type, TypedFunction, Value, imports,
};
use wasmer_vm::TrapCode;

#[derive(Default)]
struct DeltaState {
    deltas: Vec<f64>,
    index: usize,
}

impl DeltaState {
    fn next(&mut self) -> f64 {
        let value = self.deltas.get(self.index).copied().unwrap_or(0.0);
        self.index += 1;
        value
    }
}

fn jspi_module() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    const JSPI_WAT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/examples/jspi.wat");
    BYTES.get_or_init(|| wat::parse_file(JSPI_WAT).expect("valid example module"))
}

#[test]
#[cfg_attr(
    feature = "v8-default",
    ignore = "async functions are not supported by the default v8 backend"
)]
fn async_state_updates_follow_jspi_example() -> Result<()> {
    let wasm = jspi_module();
    let mut store = Store::default();
    let module = Module::new(&store, wasm)?;

    let init_state = Function::new_async(
        &mut store,
        FunctionType::new(vec![], vec![Type::F64]),
        |_values| async move {
            // Note: future::ready doesn't actually suspend. It's important
            // to note that, while we're in an async import here, it's
            // impossible to suspend during module instantiation, which is
            // where this import is called.
            // To see this in action, uncomment the following line:
            // tokio::task::yield_now().await;
            future::ready(()).await;
            Ok(vec![Value::F64(1.0)])
        },
    );

    let delta_env = FunctionEnv::new(
        &mut store,
        DeltaState {
            deltas: vec![0.5, -1.0, 2.5],
            index: 0,
        },
    );
    let compute_delta = Function::new_with_env_async(
        &mut store,
        &delta_env,
        FunctionType::new(vec![], vec![Type::F64]),
        |env: AsyncFunctionEnvMut<DeltaState>, _values| async move {
            // Note: holding a lock across an await point prevents
            // other coroutines from progressing, so it's a good
            // idea to drop the lock before awaiting.
            let delta = {
                let mut env_write = env.write().await;
                env_write.data_mut().next()
            };
            // We can, however, actually suspend whenever
            // `Function::call_async` is used to call WASM functions.
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            Ok(vec![Value::F64(delta)])
        },
    );

    let import_object = imports! {
        "js" => {
            "init_state" => init_state,
            "compute_delta" => compute_delta,
        }
    };

    let instance = Instance::new(&mut store, &module, &import_object)?;
    let get_state = instance.exports.get_function("get_state")?;
    let update_state = instance.exports.get_function("update_state")?;

    fn as_f64(values: &[Value]) -> f64 {
        match &values[0] {
            Value::F64(v) => *v,
            other => panic!("expected f64 value, got {other:?}"),
        }
    }

    assert_eq!(as_f64(&get_state.call(&mut store, &[])?), 1.0);

    let step = |store: &StoreAsync, func: &wasmer::Function| -> Result<f64> {
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(func.call_async(store, vec![]))?;
        Ok(as_f64(&result))
    };

    let store_async = store.into_async();

    assert_eq!(step(&store_async, update_state)?, 1.5);
    assert_eq!(step(&store_async, update_state)?, 0.5);
    assert_eq!(step(&store_async, update_state)?, 3.0);

    Ok(())
}

#[test]
#[cfg_attr(
    feature = "v8-default",
    ignore = "async functions are not supported by the default v8 backend"
)]
fn typed_async_host_and_calls_work() -> Result<()> {
    let wasm = wat::parse_str(
        r#"
        (module
          (import "host" "async_add" (func $async_add (param i32 i32) (result i32)))
          (import "host" "async_double" (func $async_double (param i32) (result i32)))
          (func (export "compute") (param i32) (result i32)
            local.get 0
            i32.const 10
            call $async_add
            local.get 0
            call $async_double
            i32.add))
        "#,
    )?;

    #[derive(Clone, Copy)]
    struct AddBias {
        bias: i32,
    }

    let mut store = Store::default();
    let module = Module::new(&store, wasm)?;

    let add_env = FunctionEnv::new(&mut store, AddBias { bias: 5 });
    let async_add = Function::new_typed_with_env_async(
        &mut store,
        &add_env,
        async move |env: AsyncFunctionEnvMut<AddBias>, a: i32, b: i32| {
            let env_read = env.read().await;
            let bias = env_read.data().bias;
            tokio::task::yield_now().await;
            a + b + bias
        },
    );
    let async_double = Function::new_typed_async(&mut store, async move |value: i32| {
        tokio::task::yield_now().await;
        value * 2
    });

    let import_object = imports! {
        "host" => {
            "async_add" => async_add,
            "async_double" => async_double,
        }
    };

    let instance = Instance::new(&mut store, &module, &import_object)?;
    let compute: TypedFunction<i32, i32> =
        instance.exports.get_typed_function(&store, "compute")?;

    let store_async = store.into_async();

    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(compute.call_async(&store_async, 4))?;
    assert_eq!(result, 27);

    Ok(())
}

#[test]
#[cfg_attr(
    feature = "v8-default",
    ignore = "async functions are not supported by the default v8 backend"
)]
fn cannot_yield_when_not_in_async_context() -> Result<()> {
    const WAT: &str = r#"
    (module
        (import "env" "yield_now" (func $yield_now))
        (func (export "yield_outside")
            call $yield_now
        )
    )
    "#;
    let wasm = wat::parse_str(WAT).expect("valid WAT module");

    let mut store = Store::default();
    let module = Module::new(&store, wasm)?;

    let yield_now = Function::new_async(
        &mut store,
        FunctionType::new(vec![], vec![]),
        |_values| async move {
            // Attempting to yield when not in an async context should trap.
            tokio::task::yield_now().await;
            Ok(vec![])
        },
    );

    let import_object = imports! {
        "env" => {
            "yield_now" => yield_now,
        }
    };
    let instance = Instance::new(&mut store, &module, &import_object)?;
    let yield_outside = instance.exports.get_function("yield_outside")?;

    let trap = yield_outside
        .call(&mut store, &[])
        .expect_err("expected trap calling yield outside async context");

    // TODO: wasm trace generation appears to be broken?
    // assert!(!trap.trace().is_empty(), "should have a stack trace");
    let trap_code = trap.to_trap().expect("expected trap code");
    assert_eq!(
        trap_code,
        TrapCode::YieldOutsideAsyncContext,
        "expected YieldOutsideAsyncContext trap code"
    );

    Ok(())
}

#[test]
#[cfg_attr(
    feature = "v8-default",
    ignore = "async functions are not supported by the default v8 backend"
)]
fn nested_async_in_sync() -> Result<()> {
    const WAT: &str = r#"
    (module
        (import "env" "sync" (func $sync (result i32)))
        (import "env" "async" (func $async (result i32)))
        (func (export "entry") (result i32)
            call $sync
        )
        (func (export "inner_async") (result i32)
            call $async
        )
    )
    "#;
    let wasm = wat::parse_str(WAT).expect("valid WAT module");

    let mut store = Store::default();
    let module = Module::new(&store, wasm)?;

    struct Env {
        inner_async: RefCell<Option<wasmer::TypedFunction<(), i32>>>,
    }
    let env = FunctionEnv::new(
        &mut store,
        Env {
            inner_async: RefCell::new(None),
        },
    );

    let sync = Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Env>| {
        let (env, mut store) = env.data_and_store_mut();
        env.inner_async
            .borrow()
            .as_ref()
            .expect("inner_async function to be set")
            .call(&mut store)
            .expect("inner async call to succeed")
    });

    let async_ = Function::new_typed_async(&mut store, async || {
        tokio::task::yield_now().await;
        42
    });

    let imports = imports! {
        "env" => {
            "sync" => sync,
            "async" => async_,
        }
    };

    let instance = Instance::new(&mut store, &module, &imports)?;

    let inner_async = instance
        .exports
        .get_typed_function::<(), i32>(&store, "inner_async")
        .unwrap();
    env.as_mut(&mut store)
        .inner_async
        .borrow_mut()
        .replace(inner_async);

    let entry = instance
        .exports
        .get_typed_function::<(), i32>(&store, "entry")?;
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(entry.call_async(&store.into_async()))?;

    assert_eq!(result, 42);

    Ok(())
}

/// The invariant behind [`Function::call_async`]: once the guest has been
/// entered asynchronously, every frame it reaches can still get at the async
/// context — a synchronous import included, and one that re-enters the guest and
/// suspends inside that nested call.
///
/// ```text
/// call_async -> sync import -> Function::call -> async import -> await point
/// ```
///
/// `nested_async_in_sync` covers the mechanics of that chain; this pins the
/// property the chain depends on, which is that the async context stays
/// reachable from the synchronous frame in the middle of it. Anything that
/// shadows the store's async entry while the nested call runs breaks this.
#[test]
#[cfg_attr(
    feature = "v8-default",
    ignore = "async functions are not supported by the default v8 backend"
)]
fn async_context_stays_reachable_through_a_sync_import() -> Result<()> {
    const WAT: &str = r#"
    (module
        (import "env" "sync" (func $sync (result i32)))
        (import "env" "async" (func $async (result i32)))
        (func (export "entry") (result i32)
            call $sync
        )
        (func (export "inner_async") (result i32)
            call $async
        )
    )
    "#;
    let wasm = wat::parse_str(WAT).expect("valid WAT module");

    let mut store = Store::default();
    let module = Module::new(&store, wasm)?;

    struct Env {
        inner_async: RefCell<Option<TypedFunction<(), i32>>>,
        sync_import_saw_async_context: Option<bool>,
    }
    let env = FunctionEnv::new(
        &mut store,
        Env {
            inner_async: RefCell::new(None),
            sync_import_saw_async_context: None,
        },
    );

    let sync = Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Env>| {
        // The frame in the middle of the chain: synchronous, and still inside
        // the async call that entered the guest.
        let reachable = env.as_store_async().is_some();
        let (data, mut store) = env.data_and_store_mut();
        data.sync_import_saw_async_context = Some(reachable);
        let inner_async = data
            .inner_async
            .borrow()
            .as_ref()
            .expect("inner_async function to be set")
            .clone();
        inner_async
            .call(&mut store)
            .expect("inner async call to succeed")
    });

    let async_ = Function::new_typed_async(&mut store, async || {
        tokio::task::yield_now().await;
        42
    });

    let imports = imports! {
        "env" => {
            "sync" => sync,
            "async" => async_,
        }
    };

    let instance = Instance::new(&mut store, &module, &imports)?;

    let inner_async = instance
        .exports
        .get_typed_function::<(), i32>(&store, "inner_async")
        .unwrap();
    env.as_mut(&mut store)
        .inner_async
        .borrow_mut()
        .replace(inner_async);

    let entry = instance
        .exports
        .get_typed_function::<(), i32>(&store, "entry")?;
    let store_async = store.into_async();
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(entry.call_async(&store_async))?;

    assert_eq!(result, 42);

    let store = store_async.read();
    assert_eq!(
        env.as_ref(&store).sync_import_saw_async_context,
        Some(true),
        "a synchronous import running inside Function::call_async must still \
         reach the async context"
    );

    Ok(())
}

/// Two `call_async` futures on *different* stores, driven concurrently on one
/// thread. Nothing serialises them — each store has its own lock — so their
/// store-context entries can interleave on this thread's context stack, and
/// suspending one must not disturb the other's entry.
#[test]
#[cfg_attr(
    feature = "v8-default",
    ignore = "async functions are not supported by the default v8 backend"
)]
fn two_calls_on_different_stores_interleave() -> Result<()> {
    const WAT: &str = r#"
    (module
        (import "env" "step" (func $step (result i32)))
        (func (export "run") (result i32)
            call $step
            drop
            call $step))
    "#;
    let wasm = wat::parse_str(WAT).expect("valid WAT module");

    fn build(wasm: &[u8]) -> Result<(StoreAsync, TypedFunction<(), i32>)> {
        let mut store = Store::default();
        let module = Module::new(&store, wasm)?;
        let step = Function::new_typed_async(&mut store, async || {
            tokio::task::yield_now().await;
            7
        });
        let instance = Instance::new(
            &mut store,
            &module,
            &imports! { "env" => { "step" => step } },
        )?;
        let run = instance
            .exports
            .get_typed_function::<(), i32>(&store, "run")?;
        Ok((store.into_async(), run))
    }

    let (first_store, first) = build(&wasm)?;
    let (second_store, second) = build(&wasm)?;

    let (a, b) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            futures::join!(
                first.call_async(&first_store),
                second.call_async(&second_store)
            )
        });

    assert_eq!(a?, 7);
    assert_eq!(b?, 7);
    Ok(())
}

/// The chain a suspendable dynamic call needs: an *async* import that re-enters
/// the guest with [`TypedFunction::call_async`], and a suspension inside that
/// nested call.
///
/// ```text
/// call_async -> async import -> call_async -> async import -> await point
/// ```
///
/// [`nested_async_in_sync`] covers the same chain with a synchronous import in
/// the middle, which is what WASIX's dynamic-call and lazy-binding trampolines
/// do today. That shape cannot suspend under JSPI: V8 refuses to suspend past
/// the host frame ("trying to suspend JS frames") because the nearest
/// `WebAssembly.promising` boundary sits *below* it. Making the middle import
/// async puts a boundary above that frame, which is legal — so this is the shape
/// those trampolines have to take on the JS backend, and `sys` has to keep
/// working in it too.
#[test]
#[cfg_attr(
    feature = "v8-default",
    ignore = "async functions are not supported by the default v8 backend"
)]
fn nested_async_in_async() -> Result<()> {
    const WAT: &str = r#"
    (module
        (import "env" "reenter" (func $reenter (result i32)))
        (import "env" "suspend" (func $suspend (result i32)))
        (func (export "entry") (result i32)
            call $reenter)
        (func (export "inner") (result i32)
            call $suspend))
    "#;
    let wasm = wat::parse_str(WAT).expect("valid WAT module");

    let mut store = Store::default();
    let module = Module::new(&store, wasm)?;

    type Order = Arc<Mutex<Vec<&'static str>>>;
    struct Env {
        inner: RefCell<Option<TypedFunction<(), i32>>>,
        order: Order,
    }

    let order: Order = Arc::default();
    let env = FunctionEnv::new(
        &mut store,
        Env {
            inner: RefCell::new(None),
            order: Arc::clone(&order),
        },
    );

    let reenter = Function::new_typed_with_env_async(
        &mut store,
        &env,
        async move |env: AsyncFunctionEnvMut<Env>| {
            // The read handle holds the store lock, so it has to be released
            // before the nested call, which takes that lock for itself.
            let (order, inner) = {
                let handle = env.read().await;
                let data = handle.data();
                (
                    Arc::clone(&data.order),
                    data.inner
                        .borrow()
                        .clone()
                        .expect("inner function to be set"),
                )
            };
            order.lock().unwrap().push("reenter:enter");
            // The store must be reachable *as an async store* from inside an
            // async import, or the nested call cannot be made at all.
            let store = env.as_store_async();
            let result = inner
                .call_async(&store)
                .await
                .expect("nested async call to succeed");
            order.lock().unwrap().push("reenter:leave");
            result
        },
    );

    let suspend = Function::new_typed_with_env_async(
        &mut store,
        &env,
        async move |env: AsyncFunctionEnvMut<Env>| {
            let order = Arc::clone(&env.read().await.data().order);
            order.lock().unwrap().push("suspend:before");
            tokio::task::yield_now().await;
            order.lock().unwrap().push("suspend:after");
            42
        },
    );

    let instance = Instance::new(
        &mut store,
        &module,
        &imports! {
            "env" => {
                "reenter" => reenter,
                "suspend" => suspend,
            }
        },
    )?;

    let inner = instance
        .exports
        .get_typed_function::<(), i32>(&store, "inner")?;
    env.as_mut(&mut store).inner.borrow_mut().replace(inner);

    let entry = instance
        .exports
        .get_typed_function::<(), i32>(&store, "entry")?;
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(entry.call_async(&store.into_async()))?;

    assert_eq!(result, 42);
    assert_eq!(
        *order.lock().unwrap(),
        vec![
            "reenter:enter",
            "suspend:before",
            "suspend:after",
            "reenter:leave"
        ],
        "the nested call must finish before the outer import returns"
    );

    Ok(())
}

/// [`wasmer::FunctionEnvHandle`] is how a callback registered with a foreign
/// runtime reaches its environment again, long after the call that registered it
/// returned. These pin the three outcomes it can have.
///
/// The handle holds its store weakly and is `!Send`, which matches `StoreAsync`
/// itself — an async store is single-threaded by construction — so the test
/// stashes it in a thread-local rather than in the environment.
mod function_env_handle {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static HANDLE: RefCell<Option<wasmer::FunctionEnvHandle<()>>> =
            const { RefCell::new(None) };
    }

    fn handle() -> wasmer::FunctionEnvHandle<()> {
        HANDLE
            .with(|slot| slot.borrow().clone())
            .expect("the guest registered a handle")
    }

    const WAT: &str = r#"
    (module
        (import "env" "register" (func $register))
        (import "env" "suspend" (func $suspend))
        (func (export "run")
            call $register
            call $suspend))
    "#;

    fn build() -> Result<(StoreAsync, TypedFunction<(), ()>)> {
        let wasm = wat::parse_str(WAT).expect("valid WAT module");
        let mut store = Store::default();
        let module = Module::new(&store, wasm)?;
        let env = FunctionEnv::new(&mut store, ());

        let register =
            Function::new_typed_with_env(&mut store, &env, move |env: FunctionEnvMut<()>| {
                let handle = env
                    .handle()
                    .expect("a handle, since this call came through call_async");

                // A guest is running and this frame is holding its store, so
                // nothing may hand out a second way to reach it. Refusing is the
                // whole point: granting it would be two live `StoreMut`s.
                assert!(
                    handle.try_write().is_none(),
                    "a handle must not hand out a store that is in use"
                );
                assert!(handle.is_alive(), "...but the store is alive, not gone");

                HANDLE.with(|slot| *slot.borrow_mut() = Some(handle));
            });

        let suspend = Function::new_typed_async(&mut store, async || {
            // The case that matters: a suspended guest has released its store, so
            // a callback arriving now gets exclusive access to it. This is where
            // a foreign runtime's callbacks actually land, because the guest
            // reaches its event loop by suspending into it.
            {
                let handle = handle();
                let mut guard = handle
                    .try_write()
                    .expect("a suspended guest must release its store");
                let _env_mut = guard.as_function_env_mut();
            }
            tokio::task::yield_now().await;
            // Still true after an await point.
            assert!(handle().try_write().is_some());
        });

        let instance = Instance::new(
            &mut store,
            &module,
            &imports! { "env" => { "register" => register, "suspend" => suspend } },
        )?;
        let run = instance
            .exports
            .get_typed_function::<(), ()>(&store, "run")?;
        Ok((store.into_async(), run))
    }

    /// The reachable case, and the refusal that guards it.
    #[test]
    #[cfg_attr(
        feature = "v8-default",
        ignore = "async functions are not supported by the default v8 backend"
    )]
    fn a_handle_reaches_its_store_while_the_guest_is_suspended() -> Result<()> {
        let (store, call) = build()?;
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(call.call_async(&store))?;
        Ok(())
    }

    /// Once the store is an owned `Store` again the handle is dead for good — and
    /// says so rather than panicking, which is what lets a holder drop its
    /// registration instead of queueing work that can never run.
    #[test]
    #[cfg_attr(
        feature = "v8-default",
        ignore = "async functions are not supported by the default v8 backend"
    )]
    fn a_handle_outliving_its_store_reports_it_rather_than_panicking() -> Result<()> {
        let (store, call) = build()?;
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(call.call_async(&store))?;

        let handle = handle();
        assert!(handle.is_alive(), "the store is still async here");

        let Ok(store) = store.into_store() else {
            panic!("no outstanding store clones");
        };
        drop(store);

        assert!(!handle.is_alive());
        assert!(handle.try_write().is_none());
        Ok(())
    }
}
