use super::*;
use crate::{WasiEnv, runtime::PluggableRuntime};
use wasmer::{MemoryLocation, MemoryOps, Module, Store};
use wasmer_wasix_types::wasi::ExitCode;

const MODULE: &str = r#"(module
    (import "env" "memory" (memory 1 1 shared))
    (export "memory" (memory 0)))"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn terminated_trigger_task_releases_environment_without_another_signal() {
    let manager = TokioTaskManager::new(Handle::current());
    let store = Store::default();
    let module = Module::new(&store, MODULE).unwrap();
    let mut runtime = PluggableRuntime::new(Arc::new(manager.clone()));
    runtime.set_engine(store.engine().clone());
    let env = WasiEnv::builder("termination-wakeup")
        .runtime(Arc::new(runtime))
        .build()
        .unwrap();
    let process = env.process.clone();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    // Releasing this sender also releases the worker if an assertion fails.
    let (_release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let task = TaskWasm::new(
        Box::new(move |props| {
            let result = props.trigger_result.clone().unwrap();
            drop(props);
            done_tx.send(result).unwrap();
        }),
        env,
        module,
        false,
        false,
    )
    .with_trigger(Box::new(move || {
        Box::pin(async move {
            ready_tx.send(()).unwrap();
            let _ = release_rx.await;
            Err(ExitCode::from(1))
        })
    }));
    manager.task_wasm(task).unwrap();
    ready_rx.await.unwrap();
    process.terminate(ExitCode::from(137));
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), done_rx)
            .await
            .expect("completion status must wake a pending trigger without another signal")
            .unwrap()
            .unwrap_err(),
        ExitCode::from(137)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_terminate_rejects_previously_created_queued_environments() {
    let manager = TokioTaskManager::new(Handle::current());
    let store = Store::default();
    let module = Module::new(&store, MODULE).unwrap();
    let env = WasiEnv::builder("queued-before-termination")
        .engine(store.engine().clone())
        .build()
        .unwrap();
    env.process.force_terminate(ExitCode::from(137)).unwrap();
    let task = TaskWasm::new(
        Box::new(|_| panic!("terminated guest work must not run")),
        env,
        module,
        false,
        false,
    );
    assert!(matches!(
        manager.task_wasm(task),
        Err(WasiThreadError::ProcessTerminated(code)) if code == ExitCode::from(137)
    ));
}

async fn force_terminate_during_environment_creation(has_trigger: bool) {
    let manager = TokioTaskManager::new(Handle::current());
    let store = Store::default();
    let module = Module::new(&store, MODULE).unwrap();
    let mut runtime = PluggableRuntime::new(Arc::new(manager.clone()));
    runtime.set_engine(store.engine().clone());
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<MemoryOps>();
    let ready_tx = Mutex::new(Some(ready_tx));
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let release_rx = Mutex::new(release_rx);
    runtime.with_instance_setup(move |_, store, _, imported_memory| {
        let memory = imported_memory.unwrap().as_shared(store).unwrap();
        ready_tx
            .lock()
            .unwrap()
            .take()
            .unwrap()
            .send(memory.ops())
            .unwrap();
        release_rx
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))?;
        Ok(())
    });
    let env = WasiEnv::builder("shutdown-during-instantiation")
        .runtime(Arc::new(runtime))
        .build()
        .unwrap();
    let process = env.process.clone();
    let mut task = TaskWasm::new(
        Box::new(|_| panic!("guest work must not run after shutdown during instantiation")),
        env,
        module,
        false,
        false,
    );
    if has_trigger {
        task = task.with_trigger(Box::new(|| {
            Box::pin(async { panic!("a terminated trigger must not start") })
        }));
    }
    let submit = tokio::task::spawn_blocking(move || manager.task_wasm(task));
    let memory = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();
    process.force_terminate(ExitCode::from(137)).unwrap();
    release_tx.send(()).unwrap();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), submit)
            .await
            .unwrap()
            .unwrap(),
        Err(WasiThreadError::ProcessTerminated(code)) if code == ExitCode::from(137)
    ));
    assert_eq!(process.try_join().unwrap().unwrap(), ExitCode::from(137));
    assert!(matches!(
        memory.wait(MemoryLocation::new_32(0), Some(Duration::ZERO)),
        Err(wasmer::AtomicsError::MemoryDropped)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_terminate_during_synchronous_environment_creation() {
    force_terminate_during_environment_creation(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_terminate_during_triggered_environment_creation() {
    force_terminate_during_environment_creation(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_terminate_reclaims_repeated_synchronous_guest_tasks() {
    let manager = TokioTaskManager::new(Handle::current());
    let store = Store::default();
    let module = Module::new(
        &store,
        r#"(module
            (import "env" "memory" (memory 1 1 shared))
            (export "memory" (memory 0))
            (func (export "wait") (result i32)
                (memory.atomic.wait32
                    (i32.const 0) (i32.const 0) (i64.const 3000000000))))"#,
    )
    .unwrap();
    let mut runtime = PluggableRuntime::new(Arc::new(manager.clone()));
    runtime.set_engine(store.engine().clone());
    let runtime = Arc::new(runtime);

    for _ in 0..8 {
        let parent = WasiEnv::builder("repeated-force-terminate")
            .runtime(runtime.clone())
            .build()
            .unwrap();
        let (mut child, child_handle) = parent.fork().unwrap();
        child.owned_handles.push(child_handle);
        let child_process = child.process.clone();
        // The real specific-PID syscall case is covered by process tests.
        parent.process.lock().children.clear();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let task = TaskWasm::new(
            Box::new(move |mut props| {
                let memory = props
                    .ctx
                    .data(&props.store)
                    .process
                    .lock()
                    .memory
                    .clone()
                    .unwrap();
                let wait = props
                    .ctx
                    .data(&props.store)
                    .inner()
                    .static_module_instance_handles()
                    .unwrap()
                    .instance
                    .exports
                    .get_typed_function::<(), i32>(&props.store, "wait")
                    .unwrap();
                ready_tx.send(memory).unwrap();
                let result = wait.call(&mut props.store);
                drop(props);
                done_tx.send(result.is_err()).unwrap();
            }),
            child,
            module.clone(),
            false,
            false,
        );
        manager.task_wasm(task).unwrap();
        let memory = ready_rx.await.unwrap();
        parent.process.force_terminate(ExitCode::from(137)).unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(2), done_rx)
                .await
                .expect("synchronous guest atomic wait must be interrupted")
                .unwrap()
        );
        assert_eq!(
            child_process.try_join().unwrap().unwrap(),
            ExitCode::from(137)
        );
        assert!(matches!(
            memory.wait(MemoryLocation::new_32(0), Some(Duration::ZERO)),
            Err(wasmer::AtomicsError::MemoryDropped)
        ));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_terminate_interrupts_atomic_wait_in_wasm_start_function() {
    let manager = TokioTaskManager::new(Handle::current());
    let store = Store::default();
    let module = Module::new(
        &store,
        r#"(module
            (import "env" "memory" (memory 1 1 shared))
            (import "test" "started" (func $started))
            (export "memory" (memory 0))
            (func $start
                (call $started)
                (drop (memory.atomic.wait32
                    (i32.const 0) (i32.const 0) (i64.const 3000000000))))
            (start $start))"#,
    )
    .unwrap();
    let mut runtime = PluggableRuntime::new(Arc::new(manager.clone()));
    runtime.set_engine(store.engine().clone());
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let started_tx = Arc::new(Mutex::new(Some(started_tx)));
    runtime.with_additional_imports(move |_, store| {
        let started_tx = started_tx.clone();
        let started = wasmer::Function::new_typed(store, move || {
            started_tx.lock().unwrap().take().unwrap().send(()).unwrap();
        });
        Ok(wasmer::imports! { "test" => { "started" => started } })
    });
    let env = WasiEnv::builder("force-terminate-wasm-start")
        .runtime(Arc::new(runtime))
        .build()
        .unwrap();
    let process = env.process.clone();
    let task = TaskWasm::new(
        Box::new(|_| panic!("terminated start function must not reach the run callback")),
        env,
        module,
        false,
        false,
    );
    let submit = tokio::task::spawn_blocking(move || manager.task_wasm(task));
    tokio::time::timeout(Duration::from_secs(2), started_rx)
        .await
        .unwrap()
        .unwrap();
    let memory = process
        .lock()
        .memory
        .clone()
        .expect("start memory must be registered");
    process.force_terminate(ExitCode::from(137)).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(2), submit)
            .await
            .expect("start function's atomic wait must be interrupted")
            .unwrap()
            .is_err()
    );
    assert_eq!(process.try_join().unwrap().unwrap(), ExitCode::from(137));
    assert!(matches!(
        memory.wait(MemoryLocation::new_32(0), Some(Duration::ZERO)),
        Err(wasmer::AtomicsError::MemoryDropped)
    ));
}
