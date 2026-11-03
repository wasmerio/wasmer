#![cfg(all(
    feature = "experimental-async",
    feature = "sys",
    not(target_arch = "wasm32")
))]

use std::{
    future::Future,
    pin::Pin,
    process::{Child, Command},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    time::{Duration, Instant},
};

use wasmer::{
    Function, FunctionEnv, FunctionEnvMut, FunctionType, Instance, Module, RuntimeError, Store,
    Value, imports,
};

#[test]
fn same_store_loan_preserves_split_borrows_and_expires_before_a_sibling_import() {
    same_store_loan_proof(
        "same_store_loan_preserves_split_borrows_and_expires_before_a_sibling_import",
        false,
    );
}

#[test]
fn an_escaped_store_loan_delays_canceled_stack_retirement() {
    same_store_loan_proof(
        "an_escaped_store_loan_delays_canceled_stack_retirement",
        true,
    );
}

#[test]
fn a_dynamic_host_callback_can_reenter_its_same_function_context() {
    if !run_in_subprocess("a_dynamic_host_callback_can_reenter_its_same_function_context") {
        return;
    }
    let mut store = Store::default();
    let env = FunctionEnv::new(&mut store, None::<Function>);
    let callback = Function::new_with_env(
        &mut store,
        &env,
        FunctionType::new([wasmer::Type::I32], [wasmer::Type::I32]),
        |mut env: FunctionEnvMut<Option<Function>>, values: &[Value]| {
            let count = values[0].i32().unwrap();
            if count == 0 {
                return Ok(vec![Value::I32(42)]);
            }
            let target = env.data().clone().unwrap();
            target
                .call(&mut env, &[Value::I32(count - 1)])
                .map(Vec::from)
        },
    );
    let module = Module::new(
        &store,
        r#"(module
        (import "host" "callback" (func $callback (param i32) (result i32)))
        (func (export "run") (param i32) (result i32) local.get 0 call $callback))"#,
    )
    .unwrap();
    let instance = Instance::new(
        &mut store,
        &module,
        &imports! {
            "host" => { "callback" => callback }
        },
    )
    .unwrap();
    let run = instance.exports.get_function("run").unwrap().clone();
    *env.as_mut(&mut store) = Some(run.clone());
    let result =
        futures::executor::block_on(run.call_async(&store.into_async(), vec![Value::I32(3)]))
            .unwrap();
    assert_eq!(result[0].i32(), Some(42));
}

fn same_store_loan_proof(test_name: &str, cancel: bool) {
    if !run_in_subprocess(test_name) {
        return;
    }
    use std::cell::RefCell;
    use wasmer::{
        AsStoreAsync, AsStoreMut, AsyncFunctionEnvHandleMut, AsyncFunctionEnvMut, StoreAsync,
        TypedFunction,
    };
    #[derive(Default)]
    struct Counter(i32, Option<Function>);
    struct Parent {
        leaf: Option<TypedFunction<(), i32>>,
        spin: Option<TypedFunction<(), i32>>,
        drops: Arc<AtomicUsize>,
        continued: Arc<AtomicUsize>,
        caught: Arc<AtomicUsize>,
    }
    thread_local! {
        static CAP: RefCell<Option<AsyncFunctionEnvMut<Counter>>> = const { RefCell::new(None) };
        static GUARD: RefCell<Option<AsyncFunctionEnvHandleMut<Counter>>> = const { RefCell::new(None) };
        static BASE: RefCell<Option<StoreAsync>> = const { RefCell::new(None) };
        static STOP: RefCell<Option<RuntimeError>> = const { RefCell::new(None) };
    }
    let stack_drops = Arc::new(AtomicUsize::new(0));
    let continued = Arc::new(AtomicUsize::new(0));
    let caught = Arc::new(AtomicUsize::new(0));
    let child_entries = Arc::new(AtomicUsize::new(0));
    let mut store = Store::default();
    let counter = FunctionEnv::new(&mut store, Counter::default());
    let count = child_entries.clone();
    let child = Function::new_typed(&mut store, move || -> i32 {
        count.fetch_add(1, Ordering::SeqCst);
        7
    });
    counter.as_mut(&mut store).1 = Some(child);
    let parent = FunctionEnv::new(
        &mut store,
        Parent {
            leaf: None,
            spin: None,
            drops: stack_drops.clone(),
            continued: continued.clone(),
            caught: caught.clone(),
        },
    );
    let leaf = Function::new_typed_with_env_async(
        &mut store,
        &counter,
        move |mut env: AsyncFunctionEnvMut<Counter>| async move {
            let mut write = env.write().await;
            write.data_mut().0 = 42;
            let child = write.data_mut().1.as_ref().unwrap().clone();
            let typed = child.typed::<(), i32>(&write).unwrap();
            assert_eq!(typed.call(&mut write).unwrap(), 7);
            let waker = futures::task::noop_waker();
            // This stage supports read/write lending and synchronous entry.
            // Independent child roots reject before touching this held gate.
            let mut call = Box::pin(child.call_async(&env.as_store_async(), vec![]));
            let Poll::Ready(Err(error)) = call.as_mut().poll(&mut Context::from_waker(&waker))
            else {
                panic!("borrowed child root entered or waited for the held guard");
            };
            assert!(error.message().contains("borrowed Store"));
            drop(call);
            let mut call = Box::pin(typed.call_async(&env.as_store_async()));
            let Poll::Ready(Err(error)) = call.as_mut().poll(&mut Context::from_waker(&waker))
            else {
                panic!("typed borrowed child root entered or waited for the held guard");
            };
            assert!(error.message().contains("borrowed Store"));
            drop(call);
            CAP.with(|slot| assert!(slot.borrow_mut().replace(env.as_mut()).is_none()));
            if cancel {
                GUARD.with(|slot| assert!(slot.borrow_mut().replace(write).is_none()));
            } else {
                drop(write);
                let read = env.read().await;
                assert_eq!(read.data().0, 42);
                drop(read);
                let mut first = true;
                futures::future::poll_fn(|cx| {
                    if std::mem::take(&mut first) {
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    } else {
                        Poll::Ready(())
                    }
                })
                .await;
            }
            Ok::<i32, RuntimeError>(42)
        },
    );
    let bridge = Function::new_typed_with_env(
        &mut store,
        &parent,
        move |mut env: FunctionEnvMut<Parent>| -> Result<i32, RuntimeError> {
            let (data, mut store) = env.data_and_store_mut();
            let _stack = Dropped(data.drops.clone());
            let result = data.leaf.as_ref().unwrap().call(&mut store);
            if cancel {
                let error = result.unwrap_err();
                assert!(error.is_host_abort());
                let next = data.spin.as_ref().unwrap().call(&mut store).unwrap_err();
                assert!(next.is_host_abort());
                STOP.with(|slot| {
                    let slot = slot.borrow();
                    let original = slot.as_ref().unwrap();
                    assert!(std::ptr::eq(
                        original.downcast_ref::<std::io::Error>().unwrap(),
                        error.downcast_ref::<std::io::Error>().unwrap(),
                    ));
                    assert!(std::ptr::eq(
                        original.downcast_ref::<std::io::Error>().unwrap(),
                        next.downcast_ref::<std::io::Error>().unwrap(),
                    ));
                });
                data.caught.fetch_add(1, Ordering::SeqCst);
                // Deliberately catch both errors. The import's post-body veto
                // must still prevent the calling Wasm handler or next instruction.
                return Ok(42);
            }
            let result = result?;
            data.continued.fetch_add(1, Ordering::SeqCst);
            Ok(result)
        },
    );
    let sibling = Function::new_typed_async(&mut store, |result: i32| async move {
        CAP.with(|slot| {
            let slot = slot.borrow();
            let expired = slot.as_ref().unwrap();
            assert!(expired.try_write().is_none());
            assert!(
                expired
                    .downgrade()
                    .is_some_and(|weak| weak.upgrade().is_none())
            );
        });
        // This import runs on the same still-live root after the borrowed
        // synchronous scope ended. An expired descriptor cannot fall back to
        // the now-unlocked ordinary Store handle.
        BASE.with(|slot| drop(slot.borrow().as_ref().unwrap().read()));
        result
    });
    let module = Module::new(
        &store,
        r#"(module
        (import "host" "bridge" (func $bridge (result i32)))
        (import "host" "leaf" (func $leaf (result i32)))
        (import "host" "sibling" (func $sibling (param i32) (result i32)))
        (global $continued (mut i32) (i32.const 0))
        (func (export "run") (result i32)
            (block $caught
                (try_table (catch_all $caught)
                    call $bridge
                    i32.const 1 global.set $continued
                    call $sibling return))
            i32.const 2 global.set $continued i32.const 0)
        (func (export "leaf") (result i32) call $leaf)
        (func (export "spin") (result i32) (loop $again br $again) i32.const 0)
        (func (export "continued") (result i32) global.get $continued)
        (func (export "fresh") (result i32) i32.const 9))"#,
    )
    .unwrap();
    let instance = Instance::new(
        &mut store,
        &module,
        &imports! {
            "host" => { "bridge" => bridge, "leaf" => leaf, "sibling" => sibling }
        },
    )
    .unwrap();
    parent.as_mut(&mut store).leaf =
        Some(instance.exports.get_typed_function(&store, "leaf").unwrap());
    parent.as_mut(&mut store).spin =
        Some(instance.exports.get_typed_function(&store, "spin").unwrap());
    let store = store.into_async();
    BASE.with(|slot| assert!(slot.borrow_mut().replace(store.store()).is_none()));
    let mut call = Box::pin(
        instance
            .exports
            .get_function("run")
            .unwrap()
            .call_async(&store, vec![]),
    );
    let waker = futures::task::noop_waker();
    assert!(
        call.as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_pending()
    );
    assert_eq!(stack_drops.load(Ordering::SeqCst), 0);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| store.read())).is_err(),
        "the original Store write guard must remain locked"
    );
    if cancel {
        let mut escaped = GUARD.with(|slot| slot.borrow_mut().take().unwrap());
        {
            // Derive this before cancellation. A check only in the loan guard's
            // accessor would miss this still-valid StoreMut at guest reentry.
            let mut before_cancel = escaped.as_store_mut();
            drop(call);
            let error = instance
                .exports
                .get_function("spin")
                .unwrap()
                .call(&mut before_cancel, &[])
                .unwrap_err();
            assert!(error.is_host_abort());
            STOP.with(|slot| assert!(slot.borrow_mut().replace(error).is_none()));
        }
        GUARD.with(|slot| assert!(slot.borrow_mut().replace(escaped).is_none()));
        assert_eq!(stack_drops.load(Ordering::SeqCst), 0);
        CAP.with(|slot| assert!(slot.borrow().as_ref().unwrap().try_read().is_none()));
        let mut foreign = Store::default();
        let release = Function::new_typed(&mut foreign, || -> i32 {
            drop(GUARD.with(|slot| slot.borrow_mut().take().unwrap()));
            42
        });
        let result =
            futures::executor::block_on(release.call_async(&foreign.into_async(), vec![])).unwrap();
        assert_eq!(result[0].i32(), Some(42));
        assert_eq!(continued.load(Ordering::SeqCst), 0);
        assert_eq!(caught.load(Ordering::SeqCst), 1);
        drop(STOP.with(|slot| slot.borrow_mut().take().unwrap()));
    } else {
        let result = futures::executor::block_on(call.as_mut()).unwrap();
        assert_eq!(result[0].i32(), Some(42));
        drop(call);
        assert_eq!(continued.load(Ordering::SeqCst), 1);
        assert_eq!(caught.load(Ordering::SeqCst), 0);
    }
    assert_eq!(stack_drops.load(Ordering::SeqCst), 1);
    assert_eq!(child_entries.load(Ordering::SeqCst), 1);
    assert_eq!(counter.as_ref(&store.read()).0, 42);
    let fresh = futures::executor::block_on(
        instance
            .exports
            .get_function("fresh")
            .unwrap()
            .call_async(&store, vec![]),
    )
    .unwrap();
    assert_eq!(fresh[0].i32(), Some(9));
    let guest_continued = futures::executor::block_on(
        instance
            .exports
            .get_function("continued")
            .unwrap()
            .call_async(&store, vec![]),
    )
    .unwrap();
    assert_eq!(guest_continued[0].i32(), Some(i32::from(!cancel)));
    let expired = CAP.with(|slot| slot.borrow_mut().take().unwrap());
    assert!(expired.try_write().is_none());
    let expired_store = expired.as_store_async().store();
    drop(expired);
    drop(BASE.with(|slot| slot.borrow_mut().take().unwrap()));
    drop(store);
    assert!(
        expired_store.into_store().is_err(),
        "an expired loan cannot become an ordinary Store"
    );
}

// A broken Store-installation invariant can panic while a native coroutine is
// suspended. Keep this regression in an owned subprocess so that a fail-first
// run cannot corrupt the test harness's Store TLS or unwind another test's stack.
#[test]
fn sync_cross_store_callback_can_suspend_and_resume_the_original_root() {
    cross_store_proof(
        "sync_cross_store_callback_can_suspend_and_resume_the_original_root",
        false,
        false,
    );
}

#[test]
fn typed_sync_cross_store_callback_can_suspend_and_resume_the_original_root() {
    cross_store_proof(
        "typed_sync_cross_store_callback_can_suspend_and_resume_the_original_root",
        true,
        false,
    );
}

#[test]
fn dropping_a_cross_store_callback_unwinds_its_owned_sync_stack() {
    cross_store_proof(
        "dropping_a_cross_store_callback_unwinds_its_owned_sync_stack",
        false,
        true,
    );
}

#[test]
fn dropping_a_typed_cross_store_callback_unwinds_its_owned_sync_stack() {
    cross_store_proof(
        "dropping_a_typed_cross_store_callback_unwinds_its_owned_sync_stack",
        true,
        true,
    );
}

fn run_in_subprocess(test_name: &str) -> bool {
    const CHILD: &str = "WASMER_SYNC_STORE_BORROW_CHILD";
    if std::env::var_os(CHILD).is_none() {
        struct OwnedChild(Child);
        impl Drop for OwnedChild {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = OwnedChild(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", test_name, "--nocapture"])
                .env(CHILD, "1")
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match child.0.try_wait() {
                Ok(Some(status)) => {
                    assert!(status.success(), "native Store proof failed: {status}");
                    break;
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                result => {
                    panic!("native Store proof did not finish: {result:?}");
                }
            }
        }
        return false;
    }
    true
}

struct Dropped(Arc<AtomicUsize>);

impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn cross_store_proof(test_name: &str, typed: bool, cancel: bool) {
    if !run_in_subprocess(test_name) {
        return;
    }

    let entered = Arc::new(AtomicUsize::new(0));
    let continued = Arc::new(AtomicUsize::new(0));
    let body_dropped = Arc::new(AtomicUsize::new(0));
    let stack_dropped = Arc::new(AtomicUsize::new(0));
    let (send, receive) = futures::channel::oneshot::channel::<()>();
    let receive = Arc::new(Mutex::new(Some(receive)));
    let mut store = Store::default();
    let bridge = Function::new_typed(&mut store, {
        let entered = entered.clone();
        let continued = continued.clone();
        let body_dropped = body_dropped.clone();
        let stack_dropped = stack_dropped.clone();
        move || -> Result<(), RuntimeError> {
            // B is owned on A's retained native callback stack. No Store moves
            // across threads or escapes through a pointer captured by the callback.
            let _stack = Dropped(stack_dropped.clone());
            let mut nested_store = Store::default();
            let pending = Function::new_async(&mut nested_store, FunctionType::new([], []), {
                let receive = receive.clone();
                let entered = entered.clone();
                let body_dropped = body_dropped.clone();
                move |_| {
                    entered.fetch_add(1, Ordering::SeqCst);
                    let receive = receive.lock().unwrap().take().unwrap();
                    let body = Dropped(body_dropped.clone());
                    async move {
                        let _body = body;
                        receive.await.unwrap();
                        Ok(vec![])
                    }
                }
            });
            let module = Module::new(
                &nested_store,
                r#"(module
                (import "proof" "pending" (func $pending))
                (func (export "run") call $pending))"#,
            )
            .unwrap();
            let instance = Instance::new(
                &mut nested_store,
                &module,
                &imports! { "proof" => { "pending" => pending } },
            )
            .unwrap();
            if typed {
                instance
                    .exports
                    .get_typed_function::<(), ()>(&nested_store, "run")
                    .unwrap()
                    .call(&mut nested_store)?;
            } else {
                instance
                    .exports
                    .get_function("run")
                    .unwrap()
                    .call(&mut nested_store, &[])?;
            }
            continued.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    });
    let module = Module::new(
        &store,
        r#"(module
        (import "proof" "bridge" (func $bridge))
        (func (export "run") (result i32) call $bridge i32.const 7)
        (func (export "next") (result i32) i32.const 9))"#,
    )
    .unwrap();
    let instance = Instance::new(
        &mut store,
        &module,
        &imports! { "proof" => { "bridge" => bridge } },
    )
    .unwrap();
    let store = store.into_async();
    let mut call = Box::pin(
        instance
            .exports
            .get_function("run")
            .unwrap()
            .call_async(&store, vec![]),
    );
    let waker = futures::task::noop_waker();
    assert!(
        call.as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_pending()
    );
    assert_eq!(entered.load(Ordering::SeqCst), 1);
    assert_eq!(continued.load(Ordering::SeqCst), 0);
    assert_eq!(stack_dropped.load(Ordering::SeqCst), 0);
    if cancel {
        drop(call);
        assert_eq!(continued.load(Ordering::SeqCst), 0);
    } else {
        send.send(()).unwrap();
        let result = futures::executor::block_on(call).unwrap();
        assert_eq!(result.as_ref(), &[Value::I32(7)]);
        assert_eq!(continued.load(Ordering::SeqCst), 1);
    }
    assert_eq!(body_dropped.load(Ordering::SeqCst), 1);
    assert_eq!(stack_dropped.load(Ordering::SeqCst), 1);
    let result = futures::executor::block_on(
        instance
            .exports
            .get_function("next")
            .unwrap()
            .call_async(&store, vec![]),
    )
    .unwrap();
    assert_eq!(result.as_ref(), &[Value::I32(9)]);
}

#[test]
fn a_borrowed_root_rejects_cross_store_suspension_before_yielding() {
    borrowed_root_proof(
        "a_borrowed_root_rejects_cross_store_suspension_before_yielding",
        false,
        false,
        false,
    );
}

#[test]
fn a_borrowed_root_allows_cross_store_imports_that_finish_immediately() {
    borrowed_root_proof(
        "a_borrowed_root_allows_cross_store_imports_that_finish_immediately",
        true,
        false,
        false,
    );
}

struct BorrowedImport {
    ready: bool,
    polls: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}

impl Future for BorrowedImport {
    type Output = Result<Vec<Value>, RuntimeError>;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        self.polls.fetch_add(1, Ordering::SeqCst);
        if self.ready {
            Poll::Ready(Ok(vec![]))
        } else {
            Poll::Pending
        }
    }
}

impl Drop for BorrowedImport {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn a_paused_root_rejects_cross_store_suspension_before_yielding() {
    borrowed_root_proof(
        "a_paused_root_rejects_cross_store_suspension_before_yielding",
        false,
        true,
        false,
    );
}

#[test]
fn a_paused_root_preserves_a_ready_cross_store_import() {
    borrowed_root_proof(
        "a_paused_root_preserves_a_ready_cross_store_import",
        true,
        true,
        false,
    );
}

#[test]
fn a_resumed_paused_root_still_rejects_a_cross_store_suspension() {
    borrowed_root_proof(
        "a_resumed_paused_root_still_rejects_a_cross_store_suspension",
        false,
        true,
        true,
    );
}

fn borrowed_root_proof(test_name: &str, ready: bool, paused: bool, suspend_before_cross: bool) {
    if !run_in_subprocess(test_name) {
        return;
    }
    let polls = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let (import_polls, import_drops) = (polls.clone(), drops.clone());
    let mut store = Store::default();
    let (send, receive) = futures::channel::oneshot::channel::<()>();
    let receive = Arc::new(Mutex::new(Some(receive)));
    let before_cross = Function::new_async(&mut store, FunctionType::new([], []), move |_| {
        let receive = if suspend_before_cross {
            receive.lock().unwrap().take()
        } else {
            None
        };
        async move {
            if let Some(receive) = receive {
                receive.await.unwrap();
            }
            Ok(vec![])
        }
    });
    let cross = move || -> Result<(), RuntimeError> {
        let (import_polls, import_drops) = (import_polls.clone(), import_drops.clone());
        let mut nested_store = Store::default();
        let pending =
            Function::new_async(&mut nested_store, FunctionType::new([], []), move |_| {
                BorrowedImport {
                    ready,
                    polls: import_polls.clone(),
                    drops: import_drops.clone(),
                }
            });
        let module = Module::new(
            &nested_store,
            r#"(module
                    (import "proof" "pending" (func $pending))
                    (func (export "run") call $pending))"#,
        )
        .unwrap();
        let instance = Instance::new(
            &mut nested_store,
            &module,
            &imports! { "proof" => { "pending" => pending } },
        )
        .unwrap();
        instance
            .exports
            .get_function("run")
            .unwrap()
            .call(&mut nested_store, &[])?;
        assert!(
            ready,
            "a borrowed root resumed after an inadmissible suspension"
        );
        Ok(())
    };
    let cross_direct = cross.clone();
    let cross = Function::new_typed(&mut store, cross);
    let env = FunctionEnv::new(&mut store, None::<Function>);
    let bridge = Function::new_typed_with_env(
        &mut store,
        &env,
        move |mut env: FunctionEnvMut<Option<Function>>| -> Result<(), RuntimeError> {
            if let Some(inner) = env.data().clone() {
                inner.call(&mut env, &[]).map(|_| ())
            } else {
                cross_direct()
            }
        },
    );
    let module = Module::new(
        &store,
        r#"(module
            (import "proof" "bridge" (func $bridge))
            (import "proof" "cross" (func $cross))
            (import "proof" "before_cross" (func $before_cross))
            (func (export "middle") call $before_cross call $cross)
            (func (export "run") call $bridge)
            (func (export "next") (result i32) i32.const 9))"#,
    )
    .unwrap();
    let instance = Instance::new(
        &mut store,
        &module,
        &imports! { "proof" => { "bridge" => bridge, "cross" => cross, "before_cross" => before_cross } },
    )
    .unwrap();
    if paused {
        *env.as_mut(&mut store) = Some(instance.exports.get_function("middle").unwrap().clone());
    }
    let store = store.into_async();
    let mut call = Box::pin(
        instance
            .exports
            .get_function("run")
            .unwrap()
            .call_async(&store, vec![]),
    );
    if suspend_before_cross {
        let waker = futures::task::noop_waker();
        assert!(
            call.as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        assert_eq!(polls.load(Ordering::SeqCst), 0);
        send.send(()).unwrap();
    }
    let result = futures::executor::block_on(call);
    assert_eq!(polls.load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    if ready {
        assert!(result.unwrap().is_empty());
    } else {
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("borrowed or incompatible Store")
        );
    }
    let result = futures::executor::block_on(
        instance
            .exports
            .get_function("next")
            .unwrap()
            .call_async(&store, vec![]),
    )
    .unwrap();
    assert_eq!(result.as_ref(), &[Value::I32(9)]);
}
