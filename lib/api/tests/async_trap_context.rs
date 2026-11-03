#![cfg(all(
    feature = "experimental-async",
    feature = "sys",
    not(feature = "baremetal"),
    not(target_arch = "wasm32")
))]

use std::{
    future::Future,
    process::{Child, Command},
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, Instant},
};
use wasmer::{Function, FunctionType, Instance, Module, Store, StoreAsync, imports};
use wasmer_vm::TrapCode;

// Match the owned subprocess isolation in async_sync_borrow: a missing native
// handler can turn the deliberate Wasm trap into a process signal.
fn run_in_subprocess(test_name: &str) -> bool {
    const CHILD: &str = "WASMER_ASYNC_TRAP_CONTEXT_CHILD";
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
                    assert!(
                        status.success(),
                        "native trap context proof failed: {status}"
                    );
                    break;
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                result => {
                    panic!("native trap context proof did not finish: {result:?}");
                }
            }
        }
        return false;
    }
    true
}

#[test]
fn interleaved_roots_restore_the_trap_handler_before_resuming() {
    interleaved_roots(
        "interleaved_roots_restore_the_trap_handler_before_resuming",
        false,
    );
}

#[test]
fn canceling_a_parked_root_preserves_another_roots_trap_handler() {
    interleaved_roots(
        "canceling_a_parked_root_preserves_another_roots_trap_handler",
        true,
    );
}

fn parked_instance() -> (StoreAsync, Instance, futures::channel::oneshot::Sender<()>) {
    let mut store = Store::default();
    let (send, receive) = futures::channel::oneshot::channel::<()>();
    let receive = Arc::new(Mutex::new(Some(receive)));
    let wait = Function::new_async(&mut store, FunctionType::new([], []), move |_| {
        let receive = receive.lock().unwrap().take().unwrap();
        async move {
            receive.await.unwrap();
            Ok(vec![])
        }
    });
    let module = Module::new(
        &store,
        r#"(module
        (import "proof" "wait" (func $wait))
        (memory 1 1)
        (func (export "finish") (result i32) call $wait i32.const 42)
        (func (export "trap") (param i32) call $wait local.get 0 i32.load drop)
        (func (export "fresh") (result i32) i32.const 9))"#,
    )
    .unwrap();
    let instance = Instance::new(
        &mut store,
        &module,
        &imports! { "proof" => { "wait" => wait } },
    )
    .unwrap();
    (store.into_async(), instance, send)
}

fn interleaved_roots(test_name: &str, cancel: bool) {
    if !run_in_subprocess(test_name) {
        return;
    }
    let (first_store, first_instance, first_send) = parked_instance();
    let (second_store, second_instance, second_send) = parked_instance();
    let first = first_instance.exports.get_function("finish").unwrap();
    let second = second_instance.exports.get_function("trap").unwrap();
    let mut first_call = Box::pin(first.call_async(&first_store, vec![]));
    // Cranelift's `unreachable` uses a runtime raise helper. A dynamic memory
    // load beyond this fixed memory instead needs native signal recovery.
    let mut second_call =
        Box::pin(second.call_async(&second_store, vec![wasmer::Value::I32(65536)]));
    let waker = futures::task::noop_waker();
    let mut cx = Context::from_waker(&waker);
    assert!(first_call.as_mut().poll(&mut cx).is_pending());
    assert!(second_call.as_mut().poll(&mut cx).is_pending());
    if cancel {
        drop(first_call);
        assert!(first_send.send(()).is_err());
    } else {
        first_send.send(()).unwrap();
        let Poll::Ready(Ok(result)) = first_call.as_mut().poll(&mut cx) else {
            panic!("first root did not complete");
        };
        assert_eq!(result[0].i32(), Some(42));
        drop(first_call);
    }
    second_send.send(()).unwrap();
    let Poll::Ready(Err(error)) = second_call.as_mut().poll(&mut cx) else {
        panic!("second root did not return its deliberate Wasm trap");
    };
    assert_eq!(error.to_trap(), Some(TrapCode::HeapAccessOutOfBounds));
    drop(second_call);
    for (store, instance) in [
        (&first_store, &first_instance),
        (&second_store, &second_instance),
    ] {
        let fresh = instance.exports.get_function("fresh").unwrap();
        let result = futures::executor::block_on(fresh.call_async(store, vec![])).unwrap();
        assert_eq!(result[0].i32(), Some(9));
    }
}

#[test]
#[cfg(all(unix, feature = "experimental-host-interrupt"))]
fn interrupting_a_parked_root_does_not_interrupt_the_running_foreign_root() {
    if !run_in_subprocess("interrupting_a_parked_root_does_not_interrupt_the_running_foreign_root")
    {
        return;
    }
    let (first_store, first_instance, _first_send) = parked_instance();
    // No invocation or cloned async owner exists yet. Obtain the public
    // interrupter from this exact Store before starting either root.
    let first_store = first_store.into_store().unwrap();
    let interrupter = first_store.interrupter();
    let first_store = first_store.into_async();
    let first = first_instance.exports.get_function("finish").unwrap();
    let mut first_call = Box::pin(first.call_async(&first_store, vec![]));

    let mut second_store = Store::default();
    let (send, receive) = futures::channel::oneshot::channel::<()>();
    let receive = Arc::new(Mutex::new(Some(receive)));
    let wait = Function::new_async(&mut second_store, FunctionType::new([], []), move |_| {
        let receive = receive.lock().unwrap().take().unwrap();
        async move {
            receive.await.unwrap();
            Ok(vec![])
        }
    });
    let (started_send, started_receive) = std::sync::mpsc::channel();
    let started = Function::new_typed(&mut second_store, move || {
        started_send.send(()).unwrap();
    });
    let module = Module::new(
        &second_store,
        r#"(module
        (import "proof" "wait" (func $wait))
        (import "proof" "started" (func $started))
        (memory (export "memory") 1 1 shared)
        (func (export "run") (result i32)
            call $wait call $started
            loop
                i32.const 0 i32.atomic.load i32.eqz br_if 0
            end
            i32.const 42))"#,
    )
    .unwrap();
    let instance = Instance::new(
        &mut second_store,
        &module,
        &imports! { "proof" => { "wait" => wait, "started" => started } },
    )
    .unwrap();
    let memory = instance
        .exports
        .get_memory("memory")
        .unwrap()
        .as_shared(&second_store)
        .unwrap();
    let second_store = second_store.into_async();
    let second = instance.exports.get_function("run").unwrap();
    let mut second_call = Box::pin(second.call_async(&second_store, vec![]));
    let waker = futures::task::noop_waker();
    let mut cx = Context::from_waker(&waker);
    assert!(first_call.as_mut().poll(&mut cx).is_pending());
    assert!(second_call.as_mut().poll(&mut cx).is_pending());

    let signal = std::thread::spawn(move || {
        started_receive
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        // Match interrupt.rs: allow the import to return to the Wasm hot loop,
        // and keep that loop active while the targeted signal is delivered.
        std::thread::sleep(Duration::from_millis(50));
        interrupter.interrupt();
        std::thread::sleep(Duration::from_millis(50));
        let mut host_store = Store::default();
        let memory = memory.attach(&mut host_store);
        memory
            .view(&host_store)
            .write(0, &1u32.to_le_bytes())
            .unwrap();
    });
    send.send(()).unwrap();
    let result = second_call.as_mut().poll(&mut cx);
    signal.join().unwrap();
    let Poll::Ready(Ok(result)) = result else {
        panic!("signal for the parked root affected the running foreign root");
    };
    assert_eq!(result[0].i32(), Some(42));
    drop(second_call);
    let Poll::Ready(Err(error)) = first_call.as_mut().poll(&mut cx) else {
        panic!("parked root lost its interrupt registration or flag");
    };
    assert_eq!(error.to_trap(), Some(TrapCode::HostInterrupt));
    drop(first_call);
    let fresh = first_instance.exports.get_function("fresh").unwrap();
    let result = futures::executor::block_on(fresh.call_async(&first_store, vec![])).unwrap();
    assert_eq!(result[0].i32(), Some(9));
}
