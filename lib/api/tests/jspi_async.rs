use std::{
    cell::RefCell,
    pin::Pin,
    sync::OnceLock,
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

use anyhow::Result;
use futures::{FutureExt, future};
use wasmer::{
    AsyncFunctionEnvMut, Function, FunctionEnv, FunctionType, Instance, Module, RuntimeError,
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
fn async_state_updates_follow_jspi_example() -> Result<()> {
    let wasm = jspi_module();
    let mut store = Store::default();
    let module = Module::new(&store, wasm)?;

    let init_state = Function::new_async(
        &mut store,
        FunctionType::new(vec![], vec![Type::F64]),
        |_values| async move {
            // Note: future::ready doesn't actually suspend. It's important
            // to note that, while we're in an async context here, it's
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
            let mut env_write = env.write().await;
            let delta = env_write.data_mut().next();
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
            .block_on(func.call_async(store, &[]))?;
        Ok(as_f64(&result))
    };

    let store_async = store.into_async();

    assert_eq!(step(&store_async, update_state)?, 1.5);
    assert_eq!(step(&store_async, update_state)?, 0.5);
    assert_eq!(step(&store_async, update_state)?, 3.0);

    Ok(())
}

#[test]
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
        instance.exports.get_typed_function(&mut store, "compute")?;

    let store_async = store.into_async();

    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(compute.call_async(&store_async, 4))?;
    assert_eq!(result, 27);

    Ok(())
}

// #[test]
// fn cannot_yield_when_not_in_async_context() -> Result<()> {
//     const WAT: &str = r#"
//     (module
//         (import "env" "yield_now" (func $yield_now))
//         (func (export "yield_outside")
//             call $yield_now
//         )
//     )
//     "#;
//     let wasm = wat::parse_str(WAT).expect("valid WAT module");

//     let mut store = Store::default();
//     let module = Module::new(&store, wasm)?;

//     let yield_now = Function::new_async(
//         &mut store,
//         FunctionType::new(vec![], vec![]),
//         |_values| async move {
//             // Attempting to yield when not in an async context should trap.
//             tokio::task::yield_now().await;
//             Ok(vec![])
//         },
//     );

//     let import_object = imports! {
//         "env" => {
//             "yield_now" => yield_now,
//         }
//     };
//     let instance = Instance::new(&mut store, &module, &import_object)?;
//     let yield_outside = instance.exports.get_function("yield_outside")?;

//     let trap = yield_outside
//         .call(&mut store, &[])
//         .expect_err("expected trap calling yield outside async context");

//     // TODO: wasm trace generation appears to be broken?
//     // assert!(!trap.trace().is_empty(), "should have a stack trace");
//     let trap_code = trap.to_trap().expect("expected trap code");
//     assert_eq!(
//         trap_code,
//         TrapCode::YieldOutsideAsyncContext,
//         "expected YieldOutsideAsyncContext trap code"
//     );

//     Ok(())
// }

/* This test is slightly weird to explain; what we're testing here
  is that multiple coroutines can be active at the same time,
  and that they can be polled in any order.

  To achieve this, we have 2 main imports:
   * spawn_future spawns new, pending futures, and polls them once.
     The futures are set up in a way that they will suspend the first time
     they are polled, and complete the second time. However, by polling
     once, we will kickstart the corresponding coroutine into action,
     which then stays active but suspended.
   * resolve_future polls an already spawned future a second time, which
     will cause it to be resolved. With this, we are once again activating
     the coroutine.

  We also have the future_func export and the poll_future import. future_func
  is the "body" of the inner coroutine, while poll_future retrieves the future
  constructed by spawn_future and uses it to suspend the coroutine.

  Note that the coroutine will start executing twice, once during spawn_future
  (which is a sync imported function) and once during resolve_future
  (which is async). This way we ensure that any combination of active coroutines
  is possible.

  The proper order of the log numbers for a given future is:
    * spawning the future:
      10 + id: spawn_future called initially
      20 + id: the future is constructed, but not polled yet
      30 + id: future_func polled first time
      40 + id: poll_future called, suspending the coroutine
      50 + id: spawn_future finished
    * resolving the future:
      60 + id: resolve_future called
      70 + id: future_func resumed second time
      80 + id: resolve_future finished
*/
#[test]
fn async_multiple_active_coroutines() -> Result<()> {
    const WAT: &str = r#"
    (module
        (import "env" "spawn_future" (func $spawn_future (param i32)))
        (import "env" "poll_future" (func $poll_future (param i32)))
        (import "env" "resolve_future" (func $resolve_future (param i32)))
        (import "env" "yield_now" (func $yield_now))
        (import "env" "log" (func $log (param i32)))
        (func (export "main")
            (call $spawn_future (i32.const 0))
            (call $spawn_future (i32.const 1))
            (call $yield_now)
            (call $spawn_future (i32.const 2))
            (call $resolve_future (i32.const 1))
            (call $yield_now)
            (call $spawn_future (i32.const 3))
            (call $resolve_future (i32.const 2))
            (call $yield_now)
            (call $resolve_future (i32.const 3))
            (call $resolve_future (i32.const 0))
        )
        (func (export "future_func") (param i32) (result i32)
            (call $log (i32.add (i32.const 30) (local.get 0)))
            (call $poll_future (local.get 0))
            (call $log (i32.add (i32.const 70) (local.get 0)))
            (return (local.get 0))
        )
    )
    "#;
    let wasm = wat::parse_str(WAT).expect("valid WAT module");

    struct Yielder {
        yielded: bool,
    }

    impl Future for Yielder {
        type Output = ();

        fn poll(
            mut self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            if self.yielded {
                std::task::Poll::Ready(())
            } else {
                self.yielded = true;
                std::task::Poll::Pending
            }
        }
    }

    struct Env {
        log: Vec<i32>,
        futures: [Option<Pin<Box<dyn Future<Output = Result<Box<[Value]>, RuntimeError>>>>>; 4],
        yielders: [Option<Yielder>; 4],
        future_func: Option<Function>,
    }

    let mut store = Store::default();
    let module = Module::new(&store, wasm)?;

    thread_local! {
        static ENV: RefCell<Env> = RefCell::new(Env {
            log: Vec::new(),
            futures: [None, None, None, None],
            yielders: [None, None, None, None],
            future_func: None,
        })
    }
    let mut env = FunctionEnv::new(&mut store, ());

    fn log(value: i32) {
        ENV.with(|env| {
            env.borrow_mut().log.push(value);
        });
    }

    let spawn_future = Function::new_with_env_async(
        &mut store,
        &mut env,
        FunctionType::new(vec![Type::I32], vec![]),
        |env, values| {
            let future_id = values[0].unwrap_i32();

            async move {
                ENV.with(move |data| {
                    let store_async = env.as_store_async();

                    log(future_id + 10);

                    data.borrow_mut().yielders[future_id as usize] =
                        Some(Yielder { yielded: false });

                    // This spawns the coroutine and the corresponding future
                    let func = data.borrow().future_func.as_ref().unwrap().clone();
                    let mut future = Box::pin(async move {
                        func.call_async(&store_async, &[Value::I32(future_id)])
                            .await
                    });
                    log(future_id + 20);

                    // We then poll it once to get it started - it'll suspend once, then
                    // complete the next time we poll it
                    let w = futures::task::noop_waker();
                    let mut cx = Context::from_waker(&w);
                    assert!(future.as_mut().poll(&mut cx).is_pending());

                    log(future_id + 50);
                    // We then store the future without letting it complete, and return
                    data.borrow_mut().futures[future_id as usize] = Some(future);

                    Ok(vec![])
                })
            }
        },
    );

    let poll_future = Function::new_async(
        &mut store,
        FunctionType::new(vec![Type::I32], vec![]),
        |values| {
            let future_id = values[0].unwrap_i32();
            let yielder = ENV.with(|data| {
                log(future_id + 40);

                let mut borrow = data.borrow_mut();
                let yielder = unsafe {
                    (borrow.yielders[future_id as usize].as_mut().unwrap() as *mut Yielder)
                        .as_mut::<'static>()
                        .unwrap()
                };
                yielder
            });
            yielder.map(|()| Ok(vec![]))
        },
    );

    let resolve_future = Function::new_async(
        &mut store,
        FunctionType::new(vec![Type::I32], vec![]),
        |values| {
            let future_id = values[0].unwrap_i32();

            async move {
                ENV.with(|data| {
                    log(future_id + 60);

                    let mut future = data.borrow_mut().futures[future_id as usize]
                        .take()
                        .unwrap();

                    let w = futures::task::noop_waker();
                    let mut cx = Context::from_waker(&w);
                    let Poll::Ready(result) = future.as_mut().poll(&mut cx) else {
                        panic!("expected future to be ready");
                    };
                    let result_id = result.unwrap()[0].unwrap_i32();
                    assert_eq!(result_id, future_id);

                    log(future_id + 80);

                    Ok(vec![])
                })
            }
        },
    );

    let yield_now = Function::new_async(
        &mut store,
        FunctionType::new(vec![], vec![]),
        |_values| async move {
            tokio::task::yield_now().await;
            Ok(vec![])
        },
    );

    let log = Function::new(
        &mut store,
        FunctionType::new(vec![Type::I32], vec![]),
        |values| {
            let value = values[0].unwrap_i32();
            log(value);
            Ok(vec![])
        },
    );

    let import_object = imports! {
        "env" => {
            "spawn_future" => spawn_future,
            "poll_future" => poll_future,
            "resolve_future" => resolve_future,
            "yield_now" => yield_now,
            "log" => log,
        }
    };
    let instance = Instance::new(&mut store, &module, &import_object)?;

    ENV.with(|env| {
        env.borrow_mut().future_func = Some(
            instance
                .exports
                .get_function("future_func")
                .unwrap()
                .clone(),
        )
    });

    let main = instance.exports.get_function("main")?;
    let store_async = store.into_async();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(main.call_async(&store_async, &[]))?;

    ENV.with(|env| {
        assert_eq!(
            env.borrow().log,
            vec![
                10, 20, 30, 40, 50, // future 0 spawned
                11, 21, 31, 41, 51, // future 1 spawned
                12, 22, 32, 42, 52, // future 2 spawned
                61, 71, 81, // future 1 resolved
                13, 23, 33, 43, 53, // future 3 spawned
                62, 72, 82, // future 2 resolved
                63, 73, 83, // future 3 resolved
                60, 70, 80, // future 0 resolved
            ]
        );
    });

    Ok(())
}
