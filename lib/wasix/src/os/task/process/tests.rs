use super::*;
use crate::os::task::control_plane::WasiControlPlane;

fn thread(process: &WasiProcess, main: bool) -> WasiThreadHandle {
    process
        .new_thread(
            WasiMemoryLayout::default(),
            if main {
                ThreadStartType::MainThread
            } else {
                ThreadStartType::ThreadSpawn { start_ptr: 0 }
            },
        )
        .unwrap()
}

#[tokio::test]
async fn simultaneous_any_child_waits_reap_one_exit_once() {
    let plane = WasiControlPlane::default();
    let root = plane.new_process(ModuleHash::random()).unwrap();
    let child = root.new_child(ModuleHash::random()).unwrap();
    root.lock().children.push(child.clone());
    let child_thread = thread(&child, true);

    let mut first_parent = root.clone();
    let mut second_parent = root.clone();
    let mut first = Box::pin(first_parent.join_any_child());
    let mut second = Box::pin(second_parent.join_any_child());
    assert!(matches!(
        futures::poll!(first.as_mut()),
        std::task::Poll::Pending
    ));
    assert!(matches!(
        futures::poll!(second.as_mut()),
        std::task::Poll::Pending
    ));

    child_thread.set_status_finished(Ok(ExitCode::from(23)));
    let (first, second) = futures::join!(first, second);
    let results = [first, second];
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Ok(Some(_))))
            .count(),
        1,
        "only one waiter may claim the child's exit"
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(Errno::Child)))
            .count(),
        1,
        "the second waiter must see that the child was reaped"
    );
    assert!(root.lock().children.is_empty());
}

#[tokio::test]
async fn join_children_and_any_child_cannot_both_reap_the_same_exit() {
    let plane = WasiControlPlane::default();
    let root = plane.new_process(ModuleHash::random()).unwrap();
    let child = root.new_child(ModuleHash::random()).unwrap();
    root.lock().children.push(child.clone());
    let child_thread = thread(&child, true);

    let mut all_parent = root.clone();
    let mut any_parent = root.clone();
    let mut all = Box::pin(all_parent.join_children());
    let mut any = Box::pin(any_parent.join_any_child());
    assert!(matches!(
        futures::poll!(all.as_mut()),
        std::task::Poll::Pending
    ));
    assert!(matches!(
        futures::poll!(any.as_mut()),
        std::task::Poll::Pending
    ));

    child_thread.set_status_finished(Ok(ExitCode::from(23)));
    // Let the any-child waiter claim first. The bulk waiter must not
    // report the same already-reaped child from its earlier snapshot.
    let any = any.await;
    let all = all.await;
    let all_claimed = all.is_some();
    let any_claimed = matches!(any, Ok(Some(_)));
    assert_ne!(all_claimed, any_claimed, "exactly one waiter owns the exit");
    if !any_claimed {
        assert!(matches!(any, Err(Errno::Child)));
    }
    assert!(root.lock().children.is_empty());
}

#[test]
fn force_terminate_includes_reaped_descendants_but_not_other_roots() {
    let plane = WasiControlPlane::default();
    let root = plane.new_process(ModuleHash::random()).unwrap();
    let child = root.new_child(ModuleHash::random()).unwrap();
    let grandchild = child.new_child(ModuleHash::random()).unwrap();
    let unrelated = plane.new_process(ModuleHash::random()).unwrap();
    let root_thread = thread(&root, true);
    let child_thread = thread(&child, true);
    let child_worker = thread(&child, false);
    let grandchild_thread = thread(&grandchild, true);
    let unrelated_thread = thread(&unrelated, true);

    assert_eq!(child.ppid(), root.pid());
    assert_eq!(grandchild.ppid(), child.pid());

    // Reaping is distinct from host cancellation ancestry. The main thread
    // may have exited while its workers and grandchildren are still alive.
    child_thread.set_status_finished(Ok(ExitCode::from(0)));
    root.lock().children.clear();
    child.lock().children.clear();
    let exit_code = ExitCode::from(137);
    root.force_terminate(exit_code).unwrap();

    for process in [&root, &child, &grandchild] {
        assert_eq!(process.forced_exit_code(), Some(exit_code));
        assert_eq!(
            process.new_child(ModuleHash::random()).unwrap_err(),
            ControlPlaneError::ProcessTerminated
        );
        assert_eq!(
            process
                .new_thread(WasiMemoryLayout::default(), ThreadStartType::MainThread)
                .unwrap_err(),
            ControlPlaneError::ProcessTerminated
        );
    }
    for worker in [&root_thread, &child_worker, &grandchild_thread] {
        assert_eq!(worker.try_join().unwrap().unwrap(), exit_code);
        assert!(worker.has_signal(&[Signal::Sigwakeup]));
    }
    assert_eq!(child_thread.try_join().unwrap().unwrap(), ExitCode::from(0));
    assert!(unrelated_thread.try_join().is_none());
    assert_eq!(unrelated.forced_exit_code(), None);
    assert!(unrelated.new_child(ModuleHash::random()).is_ok());
}

#[test]
fn force_terminate_rejects_child_without_changing_the_family() {
    let plane = WasiControlPlane::default();
    let root = plane.new_process(ModuleHash::random()).unwrap();
    let child = root.new_child(ModuleHash::random()).unwrap();
    let sibling = root.new_child(ModuleHash::random()).unwrap();
    let grandchild = child.new_child(ModuleHash::random()).unwrap();
    assert_eq!(
        child.force_terminate(ExitCode::from(7)),
        Err(ControlPlaneError::NotRootProcess)
    );
    assert!(grandchild.try_join().is_none());
    assert!(child.try_join().is_none());
    assert!(root.try_join().is_none());
    assert!(sibling.try_join().is_none());
}

#[test]
fn force_terminate_is_sticky_before_the_main_thread_is_created() {
    let plane = WasiControlPlane::default();
    let process = plane.new_process(ModuleHash::random()).unwrap();
    process.force_terminate(ExitCode::from(42)).unwrap();
    process.force_terminate(ExitCode::from(99)).unwrap();
    assert_eq!(process.try_join().unwrap().unwrap(), ExitCode::from(42));
    assert_eq!(process.forced_exit_code(), Some(ExitCode::from(42)));
    assert_eq!(
        process
            .new_thread_with_id(
                WasiMemoryLayout::default(),
                ThreadStartType::MainThread,
                process.pid().raw().into(),
            )
            .unwrap_err(),
        ControlPlaneError::ProcessTerminated
    );
}

#[test]
fn force_terminate_requires_the_control_plane_to_reach_the_entire_family() {
    let plane = WasiControlPlane::default();
    let process = plane.new_process(ModuleHash::random()).unwrap();
    drop(plane);
    assert_eq!(
        process.force_terminate(ExitCode::from(137)),
        Err(ControlPlaneError::Unavailable)
    );
    assert!(process.try_join().is_none());
}

#[test]
fn ordinary_terminate_does_not_force_descendants_or_close_registration() {
    let plane = WasiControlPlane::default();
    let root = plane.new_process(ModuleHash::random()).unwrap();
    let child = root.new_child(ModuleHash::random()).unwrap();
    let _root_thread = thread(&root, true);
    let child_thread = thread(&child, true);
    root.terminate(ExitCode::from(0));
    assert!(child_thread.try_join().is_none());
    assert_eq!(root.forced_exit_code(), None);
    assert!(root.new_child(ModuleHash::random()).is_ok());
    let _late_thread = thread(&root, false);
}

#[test]
fn ordinary_terminate_releases_process_lock_before_waking_completion_waiters() {
    use std::{future::Future, task::Context};

    let plane = WasiControlPlane::default();
    let process = plane.new_process(ModuleHash::random()).unwrap();
    let _main = thread(&process, true);
    let check_process = process.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let waker = waker_fn::waker_fn(move || {
        assert!(check_process.inner.0.try_lock().is_ok());
        assert!(plane.get_process(check_process.pid()).is_some());
        tx.send(()).unwrap();
    });
    let mut join = Box::pin(process.join());
    assert!(
        join.as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_pending()
    );
    process.terminate(ExitCode::from(0));
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
}

#[test]
fn sigkill_releases_process_lock_before_waking_completion_waiters() {
    use std::{future::Future, task::Context};

    let plane = WasiControlPlane::default();
    let process = plane.new_process(ModuleHash::random()).unwrap();
    let _main = thread(&process, true);
    let check_process = process.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let waker = waker_fn::waker_fn(move || {
        assert!(check_process.inner.0.try_lock().is_ok());
        assert!(plane.get_process(check_process.pid()).is_some());
        tx.send(()).unwrap();
    });
    let mut join = Box::pin(process.join());
    assert!(
        join.as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_pending()
    );
    process.signal_process(Signal::Sigkill);
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
}

#[test]
fn force_terminate_closes_every_registration_gate_before_waking_tasks() {
    let plane = WasiControlPlane::default();
    let root = plane.new_process(ModuleHash::random()).unwrap();
    let child = root.new_child(ModuleHash::random()).unwrap();
    let _root_main = thread(&root, true);
    let worker = thread(&child, false);
    let (tx, rx) = std::sync::mpsc::channel();
    let check_root = root.clone();
    let check_child = child.clone();
    let waker = waker_fn::waker_fn(move || {
        // Re-entering both locks must be safe. This also deterministically
        // attempts child/thread creation while force_terminate is delivering
        // wakeups, after its snapshot but before the parent has completed.
        assert!(plane.get_process(check_root.pid()).is_some());
        assert_eq!(
            check_root.new_child(ModuleHash::random()).unwrap_err(),
            ControlPlaneError::ProcessTerminated
        );
        assert_eq!(
            check_child
                .new_thread(WasiMemoryLayout::default(), ThreadStartType::MainThread)
                .unwrap_err(),
            ControlPlaneError::ProcessTerminated
        );
        // Ordinary exit racing the delivery phase must honor the already
        // latched force request instead of publishing a successful result.
        check_root.terminate(ExitCode::from(0));
        assert_eq!(check_root.try_join().unwrap().unwrap(), ExitCode::from(137));
        tx.send(()).unwrap();
    });
    worker.signals_subscribe(&waker);
    root.force_terminate(ExitCode::from(137)).unwrap();
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
}

#[cfg(all(feature = "sys-thread", not(target_arch = "wasm32")))]
mod native {
    use super::*;
    use wasmer::Store;
    use wasmer::{AtomicsError, MemoryLocation};
    #[cfg(all(feature = "sys", not(windows)))]
    use wasmer::{Memory, MemoryType};

    #[cfg(all(feature = "sys", not(windows)))]
    fn native_store() -> Store {
        Store::new(wasmer::sys::EngineBuilder::new(
            wasmer::sys::Cranelift::default(),
        ))
    }

    #[tokio::test]
    async fn force_terminate_rejects_vfork_child_without_disabling_parent_memory() {
        let mut store = Store::default();
        let module = wasmer::Module::new(
            &store,
            r#"(module
                (import "env" "memory" (memory 1 1 shared))
                (import "wasix_32v1" "proc_fork_env"
                    (func $fork (param i32) (result i32)))
                (export "memory" (memory 0))
                (func (export "fork") (result i32)
                    (call $fork (i32.const 0))))"#,
        )
        .unwrap();
        let (instance, env) = WasiEnv::builder("force-terminate-vfork")
            .engine(store.engine().clone())
            .instantiate(module, &mut store)
            .unwrap();
        let parent = env.data(&store).process.clone();
        instance
            .exports
            .get_typed_function::<(), i32>(&store, "fork")
            .unwrap()
            .call(&mut store)
            .unwrap();
        let child = env.data(&store).process.clone();
        assert_ne!(parent.pid(), child.pid());
        assert_eq!(
            child.force_terminate(ExitCode::from(137)),
            Err(ControlPlaneError::NotRootProcess)
        );
        let memory = instance
            .exports
            .get_memory("memory")
            .unwrap()
            .as_shared(&store)
            .unwrap();
        let has_atomic_ops = match memory.wait(MemoryLocation::new_32(32), Some(Duration::ZERO)) {
            Ok(_) => true,
            Err(AtomicsError::Unimplemented) => false,
            other => panic!("unexpected shared-memory wait result before shutdown: {other:?}"),
        };
        assert!(parent.try_join().is_none());
        assert!(child.try_join().is_none());
        parent.force_terminate(ExitCode::from(137)).unwrap();
        if has_atomic_ops {
            assert!(matches!(
                memory.wait(MemoryLocation::new_32(32), Some(Duration::ZERO)),
                Err(AtomicsError::AtomicsDisabled)
            ));
        }
        assert_eq!(child.try_join().unwrap().unwrap(), ExitCode::from(137));
    }

    // These regressions require the sys backend's host atomic-wait ops.
    // The V8 default backend exposes no SharedMemoryOps.
    #[cfg(all(feature = "sys", not(windows)))]
    #[tokio::test]
    async fn force_terminate_wakes_parent_atomics_while_joining_a_child() {
        let plane = WasiControlPlane::default();
        let mut root = plane.new_process(ModuleHash::random()).unwrap();
        let child = root.new_child(ModuleHash::random()).unwrap();
        root.lock().children.push(child.clone());
        let main = thread(&root, true);
        let sibling = thread(&root, false);
        let child_main = thread(&child, true);
        let mut store = native_store();
        let memory = Memory::new(&mut store, MemoryType::new(1, Some(1), true))
            .unwrap()
            .as_shared(&store)
            .unwrap();
        root.register_memory(memory.clone());
        let shutdown = root.clone();
        let mut join = Box::pin(root.join_any_child());
        assert!(futures::poll!(&mut join).is_pending());
        let worker_memory = memory.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let worker = tokio::task::spawn_blocking(move || {
            started_tx.send(()).unwrap();
            // The timeout ensures a failing regression does not strand a host
            // worker indefinitely during test teardown.
            worker_memory.wait(MemoryLocation::new_32(0), Some(Duration::from_secs(3)))
        });
        started_rx.await.unwrap();

        shutdown.force_terminate(ExitCode::from(137)).unwrap();
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(2), worker)
                .await
                .unwrap()
                .unwrap(),
            Err(AtomicsError::AtomicsDisabled)
        ));
        for worker in [&main, &sibling, &child_main] {
            assert_eq!(worker.try_join().unwrap().unwrap(), ExitCode::from(137));
            assert!(worker.has_signal(&[Signal::Sigwakeup]));
        }
    }

    #[cfg(all(feature = "sys", not(windows)))]
    #[test]
    fn force_terminate_disables_late_and_replacement_memories_without_retaining_them() {
        let plane = WasiControlPlane::default();
        let process = plane.new_process(ModuleHash::random()).unwrap();
        process.force_terminate(ExitCode::from(137)).unwrap();
        for _ in 0..8 {
            let ops = {
                let mut store = native_store();
                let memory = Memory::new(&mut store, MemoryType::new(1, Some(1), true))
                    .unwrap()
                    .as_shared(&store)
                    .unwrap();
                let ops = memory.ops();
                process.register_memory(memory);
                assert!(matches!(
                    ops.wait(MemoryLocation::new_32(0), Some(Duration::ZERO)),
                    Err(AtomicsError::AtomicsDisabled)
                ));
                ops
            };
            assert!(matches!(
                ops.wait(MemoryLocation::new_32(0), Some(Duration::ZERO)),
                Err(AtomicsError::MemoryDropped)
            ));
        }
    }

    #[tokio::test]
    async fn nonblocking_proc_join_keeps_live_child_waitable_and_force_terminable() {
        let mut store = Store::default();
        let module = wasmer::Module::new(
            &store,
            r#"(module
                (import "env" "memory" (memory 1 1 shared))
                (import "wasix_32v1" "proc_join"
                    (func $proc_join (param i32 i32 i32) (result i32)))
                (export "memory" (memory 0))
                (func (export "join_child") (param $pid i32) (result i32)
                    (i32.store8 (i32.const 0) (i32.const 1))
                    (i32.store (i32.const 4) (local.get $pid))
                    (call $proc_join (i32.const 0) (i32.const 1) (i32.const 16)))
                (func (export "joined_pid_tag") (result i32)
                    (i32.load8_u (i32.const 0)))
                (func (export "joined_pid") (result i32)
                    (i32.load (i32.const 4)))
                (func (export "join_status_tag") (result i32)
                    (i32.load8_u (i32.const 16))))"#,
        )
        .unwrap();
        let (instance, env) = WasiEnv::builder("force-terminate-proc-join")
            .engine(store.engine().clone())
            .instantiate(module, &mut store)
            .unwrap();
        let parent = env.data(&store).process.clone();
        let (child_env, _child_handle) = env.data(&store).fork().unwrap();
        let child = child_env.process.clone();
        parent.lock().children.push(child.clone());
        assert_eq!(parent.lock().children.len(), 1);
        assert_eq!(
            instance
                .exports
                .get_typed_function::<i32, i32>(&store, "join_child")
                .unwrap()
                .call(&mut store, child.pid().raw() as i32)
                .unwrap(),
            Errno::Success as i32
        );
        assert_eq!(
            instance
                .exports
                .get_typed_function::<(), i32>(&store, "joined_pid_tag")
                .unwrap()
                .call(&mut store)
                .unwrap(),
            wasmer_wasix_types::wasi::OptionTag::Some as i32
        );
        assert_eq!(
            instance
                .exports
                .get_typed_function::<(), i32>(&store, "joined_pid")
                .unwrap()
                .call(&mut store)
                .unwrap(),
            0
        );
        assert_eq!(
            instance
                .exports
                .get_typed_function::<(), i32>(&store, "join_status_tag")
                .unwrap()
                .call(&mut store)
                .unwrap(),
            wasmer_wasix_types::wasi::JoinStatusType::Nothing as i32
        );
        assert_eq!(parent.lock().children.len(), 1);
        assert!(child.try_join().is_none());
        parent.force_terminate(ExitCode::from(137)).unwrap();
        assert_eq!(child.try_join().unwrap().unwrap(), ExitCode::from(137));
    }

    #[tokio::test]
    async fn nonblocking_proc_join_reaps_only_after_child_exit() {
        let mut store = Store::default();
        let module = wasmer::Module::new(
            &store,
            r#"(module
                (import "env" "memory" (memory 1 1 shared))
                (import "wasix_32v1" "proc_join"
                    (func $proc_join (param i32 i32 i32) (result i32)))
                (export "memory" (memory 0))
                (func (export "join_child") (param $pid i32) (result i32)
                    (i32.store8 (i32.const 0) (i32.const 1))
                    (i32.store (i32.const 4) (local.get $pid))
                    (call $proc_join (i32.const 0) (i32.const 1) (i32.const 16)))
                (func (export "join_child_blocking") (param $pid i32) (result i32)
                    (i32.store8 (i32.const 0) (i32.const 1))
                    (i32.store (i32.const 4) (local.get $pid))
                    (call $proc_join (i32.const 0) (i32.const 0) (i32.const 16)))
                (func (export "join_any") (result i32)
                    (i32.store8 (i32.const 0) (i32.const 0))
                    (call $proc_join (i32.const 0) (i32.const 1) (i32.const 16)))
                (func (export "join_bad_pid") (result i32)
                    (call $proc_join (i32.const 65536) (i32.const 1) (i32.const 16)))
                (func (export "join_bad_status") (param $pid i32) (result i32)
                    (i32.store8 (i32.const 0) (i32.const 1))
                    (i32.store (i32.const 4) (local.get $pid))
                    (call $proc_join (i32.const 0) (i32.const 1) (i32.const 65536)))
                (func (export "joined_pid_tag") (result i32)
                    (i32.load8_u (i32.const 0)))
                (func (export "joined_pid") (result i32)
                    (i32.load (i32.const 4)))
                (func (export "join_status_tag") (result i32)
                    (i32.load8_u (i32.const 16)))
                (func (export "join_exit_code") (result i32)
                    (i32.load16_u (i32.const 18))))"#,
        )
        .unwrap();
        let (instance, env) = WasiEnv::builder("nonblocking-proc-join")
            .engine(store.engine().clone())
            .instantiate(module, &mut store)
            .unwrap();
        let parent = env.data(&store).process.clone();
        let (child_env, child_handle) = env.data(&store).fork().unwrap();
        let child = child_env.process.clone();
        parent.lock().children.push(child.clone());
        let join = instance
            .exports
            .get_typed_function::<i32, i32>(&store, "join_child")
            .unwrap();
        let join_blocking = instance
            .exports
            .get_typed_function::<i32, i32>(&store, "join_child_blocking")
            .unwrap();
        let join_any = instance
            .exports
            .get_typed_function::<(), i32>(&store, "join_any")
            .unwrap();
        let pid_tag = instance
            .exports
            .get_typed_function::<(), i32>(&store, "joined_pid_tag")
            .unwrap();
        let joined_pid = instance
            .exports
            .get_typed_function::<(), i32>(&store, "joined_pid")
            .unwrap();
        let status_tag = instance
            .exports
            .get_typed_function::<(), i32>(&store, "join_status_tag")
            .unwrap();
        let exit_code = instance
            .exports
            .get_typed_function::<(), i32>(&store, "join_exit_code")
            .unwrap();

        assert_eq!(
            join.call(&mut store, child.pid().raw() as i32).unwrap(),
            Errno::Success as i32
        );
        assert_eq!(
            pid_tag.call(&mut store).unwrap(),
            wasmer_wasix_types::wasi::OptionTag::Some as i32
        );
        assert_eq!(joined_pid.call(&mut store).unwrap(), 0);
        assert_eq!(
            status_tag.call(&mut store).unwrap(),
            wasmer_wasix_types::wasi::JoinStatusType::Nothing as i32
        );
        assert_eq!(parent.lock().children.len(), 1);
        assert_eq!(join_any.call(&mut store).unwrap(), Errno::Success as i32);
        assert_eq!(
            pid_tag.call(&mut store).unwrap(),
            wasmer_wasix_types::wasi::OptionTag::Some as i32
        );
        assert_eq!(joined_pid.call(&mut store).unwrap(), 0);
        assert_eq!(parent.lock().children.len(), 1);

        assert_eq!(
            instance
                .exports
                .get_typed_function::<(), i32>(&store, "join_bad_pid")
                .unwrap()
                .call(&mut store)
                .unwrap(),
            Errno::Memviolation as i32
        );
        assert_eq!(
            instance
                .exports
                .get_typed_function::<i32, i32>(&store, "join_bad_status")
                .unwrap()
                .call(&mut store, child.pid().raw() as i32)
                .unwrap(),
            Errno::Memviolation as i32
        );
        assert_eq!(parent.lock().children.len(), 1);

        // A process in the same control plane is not necessarily our child.
        // Only children on this parent's reap list may be joined.
        let unrelated = env
            .data(&store)
            .control_plane
            .new_process(ModuleHash::random())
            .unwrap();
        assert_eq!(
            join.call(&mut store, unrelated.pid().raw() as i32).unwrap(),
            Errno::Child as i32
        );
        assert_eq!(
            pid_tag.call(&mut store).unwrap(),
            wasmer_wasix_types::wasi::OptionTag::None as i32
        );

        child_handle.set_status_finished(Ok(ExitCode::from(23)));
        // Bad outputs must not consume an exit that is ready to be reaped.
        assert_eq!(
            instance
                .exports
                .get_typed_function::<(), i32>(&store, "join_bad_pid")
                .unwrap()
                .call(&mut store)
                .unwrap(),
            Errno::Memviolation as i32
        );
        assert_eq!(
            instance
                .exports
                .get_typed_function::<i32, i32>(&store, "join_bad_status")
                .unwrap()
                .call(&mut store, child.pid().raw() as i32)
                .unwrap(),
            Errno::Memviolation as i32
        );
        assert_eq!(parent.lock().children.len(), 1);
        assert_eq!(
            join.call(&mut store, child.pid().raw() as i32).unwrap(),
            Errno::Success as i32
        );
        assert_eq!(
            pid_tag.call(&mut store).unwrap(),
            wasmer_wasix_types::wasi::OptionTag::Some as i32
        );
        assert_eq!(
            status_tag.call(&mut store).unwrap(),
            wasmer_wasix_types::wasi::JoinStatusType::ExitNormal as i32
        );
        assert_eq!(exit_code.call(&mut store).unwrap(), 23);
        assert_eq!(
            joined_pid.call(&mut store).unwrap(),
            child.pid().raw() as i32
        );
        assert!(parent.lock().children.is_empty());
        assert_eq!(
            join.call(&mut store, child.pid().raw() as i32).unwrap(),
            Errno::Child as i32
        );
        assert_eq!(
            pid_tag.call(&mut store).unwrap(),
            wasmer_wasix_types::wasi::OptionTag::None as i32
        );
        assert_eq!(join_any.call(&mut store).unwrap(), Errno::Child as i32);

        // Exercise both claim orders through the blocking PID-specific syscall
        // and the any-child poll. Only the first syscall may report the exit.
        let (second_env, second_handle) = env.data(&store).fork().unwrap();
        let second = second_env.process.clone();
        parent.lock().children.push(second.clone());
        second_handle.set_status_finished(Ok(ExitCode::from(19)));
        assert_eq!(
            join_blocking
                .call(&mut store, second.pid().raw() as i32)
                .unwrap(),
            Errno::Success as i32
        );
        assert_eq!(
            joined_pid.call(&mut store).unwrap(),
            second.pid().raw() as i32
        );
        assert_eq!(
            status_tag.call(&mut store).unwrap(),
            wasmer_wasix_types::wasi::JoinStatusType::ExitNormal as i32
        );
        assert_eq!(exit_code.call(&mut store).unwrap(), 19);
        assert!(parent.lock().children.is_empty());
        assert_eq!(join_any.call(&mut store).unwrap(), Errno::Child as i32);
        assert_eq!(
            pid_tag.call(&mut store).unwrap(),
            wasmer_wasix_types::wasi::OptionTag::None as i32
        );

        let (third_env, third_handle) = env.data(&store).fork().unwrap();
        let third = third_env.process.clone();
        parent.lock().children.push(third.clone());
        third_handle.set_status_finished(Ok(ExitCode::from(17)));
        assert_eq!(join_any.call(&mut store).unwrap(), Errno::Success as i32);
        assert_eq!(
            joined_pid.call(&mut store).unwrap(),
            third.pid().raw() as i32
        );
        assert_eq!(
            status_tag.call(&mut store).unwrap(),
            wasmer_wasix_types::wasi::JoinStatusType::ExitNormal as i32
        );
        assert_eq!(exit_code.call(&mut store).unwrap(), 17);
        assert!(parent.lock().children.is_empty());
        assert_eq!(
            join_blocking
                .call(&mut store, third.pid().raw() as i32)
                .unwrap(),
            Errno::Child as i32
        );
        assert_eq!(
            pid_tag.call(&mut store).unwrap(),
            wasmer_wasix_types::wasi::OptionTag::None as i32
        );

        assert_eq!(
            join.call(&mut store, i32::MAX).unwrap(),
            Errno::Child as i32
        );
        assert_eq!(
            pid_tag.call(&mut store).unwrap(),
            wasmer_wasix_types::wasi::OptionTag::None as i32
        );
    }
}
