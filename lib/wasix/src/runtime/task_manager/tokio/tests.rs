use super::*;
use crate::{WasiEnv, runtime::PluggableRuntime};
use std::sync::atomic::Ordering;
use wasmer::{AtomicsError, MemoryLocation, Module, Store};
use wasmer_wasix_types::{types::Signal, wasi::Errno};

const SHARED_MEMORY_MODULE: &str = r#"(module
    (import "env" "memory" (memory 1 1 shared))
    (export "memory" (memory 0)))"#;

async fn assert_process_sigkill_reclaims_trigger_task(handler_registered: bool) {
    let manager = TokioTaskManager::new(Handle::current());
    let store = Store::default();
    let module = Module::new(&store, SHARED_MEMORY_MODULE).unwrap();
    let mut runtime = PluggableRuntime::new(Arc::new(manager.clone()));
    runtime.set_engine(store.engine().clone());
    let env = WasiEnv::builder("sigkill-process-handler")
        .runtime(Arc::new(runtime))
        .build()
        .unwrap();
    // A callback lives on the instance that registered it, while sibling
    // instances only observe this process-wide flag.
    env.state
        .signal_handler_registered
        .store(handler_registered, Ordering::SeqCst);
    let process = env.process.clone();

    let (trigger_ready_tx, trigger_ready_rx) = tokio::sync::oneshot::channel();
    let (_keep_trigger_pending, trigger_pending_rx) = tokio::sync::oneshot::channel::<()>();
    let (task_done_tx, task_done_rx) = tokio::sync::oneshot::channel();
    let task = TaskWasm::new(
        Box::new(move |properties| {
            let result = properties.trigger_result.clone().unwrap();
            drop(properties);
            let _ = task_done_tx.send(result);
        }),
        env,
        module,
        false,
        false,
    )
    .with_trigger(Box::new(move || {
        Box::pin(async move {
            trigger_ready_tx.send(()).unwrap();
            let _ = trigger_pending_rx.await;
            Ok(Vec::new().into())
        })
    }));

    manager.task_wasm(task).unwrap();
    trigger_ready_rx.await.unwrap();
    let memory = process
        .lock()
        .memory
        .clone()
        .expect("task memory must be registered before its trigger starts");
    let waiter_memory = memory.clone();
    let (waiter_ready_tx, waiter_ready_rx) = tokio::sync::oneshot::channel();
    let waiter = tokio::task::spawn_blocking(move || {
        waiter_ready_tx.send(()).unwrap();
        waiter_memory.wait(MemoryLocation::new_32(0), None)
    });
    waiter_ready_rx.await.unwrap();

    process.signal_process(Signal::Sigkill);

    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), task_done_rx)
            .await
            .expect("SIGKILL must release the pending trigger task")
            .unwrap()
            .unwrap_err(),
        Errno::Intr.into()
    );
    assert!(matches!(
        waiter.await.unwrap(),
        Err(AtomicsError::AtomicsDisabled | AtomicsError::MemoryDropped)
    ));
    assert_eq!(process.try_join().unwrap().unwrap(), Errno::Intr.into());
    assert!(matches!(
        memory.wait(MemoryLocation::new_32(0), Some(Duration::ZERO)),
        Err(AtomicsError::MemoryDropped)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn process_sigkill_reclaims_trigger_task_with_process_signal_handler() {
    assert_process_sigkill_reclaims_trigger_task(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn process_sigkill_reclaims_trigger_task_without_signal_handler() {
    assert_process_sigkill_reclaims_trigger_task(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn process_sigkill_interrupts_guest_atomic_wait() {
    let manager = TokioTaskManager::new(Handle::current());
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();

    tokio::task::spawn_blocking(move || {
        let mut store = Store::default();
        let module = Module::new(
            &store,
            r#"(module
                (import "env" "memory" (memory 1 1 shared))
                (export "memory" (memory 0))
                (func (export "wait") (result i32)
                    (memory.atomic.wait32
                        (i32.const 0) (i32.const 0) (i64.const -1))))"#,
        )
        .unwrap();
        let mut runtime = PluggableRuntime::new(Arc::new(manager));
        runtime.set_engine(store.engine().clone());
        let (instance, env) = WasiEnv::builder("sigkill-guest-atomic-wait")
            .runtime(Arc::new(runtime))
            .instantiate(module, &mut store)
            .unwrap();
        let process = env.data(&store).process.clone();
        let memory = instance
            .exports
            .get_memory("memory")
            .unwrap()
            .as_shared(&store)
            .unwrap();
        ready_tx.send((process, memory.ops())).unwrap();
        let interrupted = instance
            .exports
            .get_typed_function::<(), i32>(&store, "wait")
            .unwrap()
            .call(&mut store)
            .is_err();
        drop(memory);
        drop(instance);
        drop(store);
        done_tx.send(interrupted).unwrap();
    });

    let (process, memory) = ready_rx.await.unwrap();
    process.signal_process(Signal::Sigkill);
    let done = tokio::time::timeout(Duration::from_secs(2), done_rx).await;
    if done.is_err() {
        let _ = memory.disable_atomics();
    }
    assert!(
        done.expect("SIGKILL must interrupt a guest atomic wait")
            .unwrap()
    );
    assert_eq!(process.try_join().unwrap().unwrap(), Errno::Intr.into());
    assert!(matches!(
        memory.wait(MemoryLocation::new_32(0), Some(Duration::ZERO)),
        Err(AtomicsError::MemoryDropped)
    ));
}
