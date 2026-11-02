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
