use std::{
    collections::{BTreeMap, HashMap},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use wasmer::{Global, Memory, MemoryType, Module, Store, Table, TableType, Tag, Type, Value};

use super::*;
use crate::state::linker::{DylinkInfo, MemoryAllocator};

const DEADLINE: Duration = Duration::from_secs(2);

fn fixture(store: &Store) -> LinkerShared {
    LinkerShared::new(
        LinkerState {
            engine: store.engine().clone(),
            main_module: Module::new(store, "(module)").unwrap(),
            main_module_dylink_info: DylinkInfo {
                mem_info: wasmparser::MemInfo {
                    memory_size: 0,
                    memory_alignment: 0,
                    table_size: 0,
                    table_alignment: 0,
                },
                needed: vec![],
                import_metadata: HashMap::new(),
                export_metadata: HashMap::new(),
                runtime_path: vec![],
            },
            main_module_memory_base: 0,
            side_modules: BTreeMap::new(),
            side_modules_by_name: HashMap::new(),
            next_module_handle: 2,
            memory_allocator: MemoryAllocator::new(),
            heap_base: 0,
            allocated_closure_functions: BTreeMap::new(),
            available_closure_functions: vec![],
            symbol_resolution_records: HashMap::new(),
            send_pending_operation_barrier: bus::Bus::new(1),
            send_pending_operation: bus::Bus::new(1),
        },
        LinkerCancellation::new(),
    )
}

fn group(
    store: &mut Store,
    recv_pending_operation_barrier: bus::BusReader<Arc<Barrier>>,
    recv_pending_operation: bus::BusReader<DlOperation>,
) -> InstanceGroupState {
    InstanceGroupState {
        main_instance: None,
        main_instance_tls_base: None,
        side_instances: HashMap::new(),
        stack_pointer: Global::new_mut(store, Value::I32(0)),
        memory: Memory::new(store, MemoryType::new(1, Some(1), true)).unwrap(),
        indirect_function_table: Table::new(
            store,
            TableType::new(Type::FuncRef, 1, None),
            Value::FuncRef(None),
        )
        .unwrap(),
        c_longjmp: Tag::new(store, vec![Type::I32]),
        cpp_exception: Tag::new(store, vec![Type::I32]),
        recv_pending_operation_barrier,
        recv_pending_operation,
    }
}

fn env(store: &Store) -> WasiEnv {
    WasiEnv::builder("linker-round-test")
        .engine(store.engine().clone())
        .build()
        .unwrap()
}

fn assert_aborted<T>(result: Result<T, LinkError>) {
    assert!(matches!(result, Err(LinkError::SynchronizationAborted(code)) if code == 137.into()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn leader_abort_releases_rendezvous_and_waiting_topology_contender() {
    let store = Store::default();
    let shared = fixture(&store);
    let leader_env = env(&store);
    let leader_process = leader_env.process.clone();
    let follower_env = env(&store);
    let (leader_barrier, leader_op, follower_barrier, follower_op, _missing_barrier, _missing_op) = {
        let mut state = shared.linker_state.write().unwrap();
        (
            state.send_pending_operation_barrier.add_rx(),
            state.send_pending_operation.add_rx(),
            state.send_pending_operation_barrier.add_rx(),
            state.send_pending_operation.add_rx(),
            state.send_pending_operation_barrier.add_rx(),
            state.send_pending_operation.add_rx(),
        )
    };
    let topology = shared.topology_coordinator.try_acquire().unwrap();
    let leader_shared = shared.clone();
    let (done_tx, done_rx) = mpsc::channel();
    let leader_done = done_tx.clone();
    let leader = thread::spawn(move || {
        let mut store = Store::default();
        let mut group = group(&mut store, leader_barrier, leader_op);
        let (topology, state) = leader_shared
            .write_linker_state_holding_topology(topology, &leader_env)
            .unwrap();
        let result = leader_shared.synchronize_link_operation(
            topology,
            DlOperation::AllocateFunctionTable { index: 1, size: 1 },
            state,
            &mut group,
            &leader_env,
        );
        let _ = leader_done.send(result);
    });
    let deadline = Instant::now() + DEADLINE;
    while !shared.dl_operation_pending_load(Ordering::SeqCst) {
        assert!(
            Instant::now() < deadline,
            "leader never published its rendezvous"
        );
        thread::yield_now();
    }
    let follower_shared = shared.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let follower = thread::spawn(move || {
        let mut store = Store::default();
        let mut group = group(&mut store, follower_barrier, follower_op);
        let env = FunctionEnv::new(&mut store, follower_env);
        entered_tx.send(()).unwrap();
        let result = follower_shared
            .acquire_topology_token(&mut group, &mut store, &env)
            .map(drop);
        let _ = done_tx.send(result);
    });
    entered_rx.recv_timeout(DEADLINE).unwrap();
    leader_process.force_terminate(137.into()).unwrap();
    // Both real LinkerShared paths must return despite the missing third group.
    let first = done_rx
        .recv_timeout(DEADLINE)
        .expect("leader/follower retained rendezvous");
    let second = done_rx
        .recv_timeout(DEADLINE)
        .expect("topology contender retained rendezvous");
    leader.join().unwrap();
    follower.join().unwrap();
    assert_aborted(first);
    assert_aborted(second);
    // The lease is actually released, but the poisoned linker refuses new work.
    let topology = shared
        .topology_coordinator
        .try_acquire()
        .expect("aborted leader retained topology lease");
    assert_aborted(shared.write_linker_state_holding_topology(topology, &env(&store)));
    assert!(shared.topology_coordinator.try_acquire().is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn aborted_linker_rejects_read_and_topology_write_even_with_free_locks() {
    let store = Store::default();
    let shared = fixture(&store);
    let env = env(&store);
    env.process.force_terminate(137.into()).unwrap();
    assert_aborted(shared.read_linker_state(&env));
    let topology = shared.topology_coordinator.try_acquire().unwrap();
    assert_aborted(shared.write_linker_state_holding_topology(topology, &env));
    assert!(shared.topology_coordinator.try_acquire().is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn poisoned_linker_traps_guest_before_any_post_syscall_instruction() {
    let mut store = Store::default();
    let shared = fixture(&store);
    let module = Module::new(
        &store,
        r#"(module
        (import "wasix_32v1" "dl_invalid_handle" (func $check (param i32) (result i32)))
        (memory (export "memory") 1)
        (func (export "_start")
            (drop (call $check (i32.const 1)))
            (i32.store (i32.const 0) (i32.const 99))))"#,
    )
    .unwrap();
    let (instance, env) = WasiEnv::builder("guest-poisoned-linker")
        .engine(store.engine().clone())
        .instantiate(module, &mut store)
        .unwrap();
    let handles = env
        .data(&store)
        .inner()
        .main_module_instance_handles()
        .clone();
    let linker = crate::state::linker::Linker {
        shared: shared.clone(),
        instance_group_state: Arc::new(std::sync::Mutex::new(None)),
    };
    env.data_mut(&mut store)
        .set_inner(crate::WasiModuleTreeHandles::Dynamic {
            linker,
            main_module_instance_handles: handles,
        });
    // This is linker poison alone, not a process status check masking a missed
    // fatal error. The pending-operation flag is also false on this fast path.
    assert!(env.data(&store).should_exit().is_none());
    let _ = shared.cancellation.abort(137.into());
    let start = instance
        .exports
        .get_typed_function::<(), ()>(&store, "_start")
        .unwrap();
    let error = start
        .call(&mut store)
        .expect_err("poisoned linker returned a guest errno");
    assert!(matches!(error.downcast_ref::<crate::WasiError>(),
        Some(crate::WasiError::Exit(code)) if *code == 137.into()));
    let memory = instance.exports.get_memory("memory").unwrap();
    assert_eq!(
        memory.view(&store).read_u8(0).unwrap(),
        0,
        "guest continued after a partially applied dynamic-link transaction"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cached_stub_cannot_reenter_guest_after_linker_abort() {
    use wasmer::{FunctionType, Instance, imports};
    let mut store = Store::default();
    let shared = fixture(&store);
    let module = Module::new(
        &store,
        r#"(module
        (memory (export "memory") 1)
        (global $calls (export "calls") (mut i32) (i32.const 0))
        (func (export "target") (result i32)
            (global.set $calls (i32.add (global.get $calls) (i32.const 1)))
            (global.get $calls)))"#,
    )
    .unwrap();
    let instance = Instance::new(&mut store, &module, &imports! {}).unwrap();
    let (barrier, operation) = {
        let mut state = shared.linker_state.write().unwrap();
        (
            state.send_pending_operation_barrier.add_rx(),
            state.send_pending_operation.add_rx(),
        )
    };
    let mut group = group(&mut store, barrier, operation);
    group.main_instance = Some(instance.clone());
    let linker = crate::state::linker::Linker {
        shared: shared.clone(),
        instance_group_state: Arc::new(std::sync::Mutex::new(Some(group))),
    };
    let mut wasi_env = env(&store);
    let handles = crate::WasiModuleInstanceHandles::new(
        instance.exports.get_memory("memory").unwrap().clone(),
        &store,
        instance.clone(),
        None,
    );
    wasi_env.set_inner(crate::WasiModuleTreeHandles::Dynamic {
        linker: linker.clone(),
        main_module_instance_handles: handles,
    });
    let func_env = FunctionEnv::new(&mut store, wasi_env);
    let stub = linker
        .instance_group_state
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .generate_stub_function(
            &mut store,
            &FunctionType::new([], [Type::I32]),
            &func_env,
            crate::state::linker::MAIN_MODULE_HANDLE,
            "target".into(),
        );
    assert_eq!(stub.call(&mut store, &[]).unwrap()[0], Value::I32(1));
    let _ = shared.cancellation.abort(137.into());
    let error = stub
        .call(&mut store, &[])
        .expect_err("cached stub bypassed linker abort");
    assert!(matches!(error.downcast_ref::<crate::WasiError>(),
        Some(crate::WasiError::Exit(code)) if *code == 137.into()));
    assert_eq!(
        instance
            .exports
            .get_global("calls")
            .unwrap()
            .get(&mut store),
        Value::I32(1)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn healthy_group_shutdown_allows_subsequent_two_epoch_replays() {
    let mut store = Store::default();
    let shared = fixture(&store);
    let (leader_barrier, leader_op, follower_barrier, follower_op, closing_barrier, closing_op) = {
        let mut state = shared.linker_state.write().unwrap();
        (
            state.send_pending_operation_barrier.add_rx(),
            state.send_pending_operation.add_rx(),
            state.send_pending_operation_barrier.add_rx(),
            state.send_pending_operation.add_rx(),
            state.send_pending_operation_barrier.add_rx(),
            state.send_pending_operation.add_rx(),
        )
    };
    let closing = crate::state::linker::Linker {
        shared: shared.clone(),
        instance_group_state: Arc::new(std::sync::Mutex::new(Some(group(
            &mut store,
            closing_barrier,
            closing_op,
        )))),
    };
    let closing_env = env(&store);
    let closing_env = FunctionEnv::new(&mut store, closing_env);
    closing
        .shutdown_instance_group(&mut closing_env.into_mut(&mut store))
        .unwrap();
    assert!(closing.instance_group_state.lock().unwrap().is_none());
    assert_eq!(
        shared
            .linker_state
            .read()
            .unwrap()
            .send_pending_operation
            .rx_count(),
        2
    );

    let leader_env = env(&store);
    let leader_process = leader_env.process.clone();
    let follower_env = env(&store);
    let follower_process = follower_env.process.clone();
    let leader_shared = shared.clone();
    let follower_shared = shared.clone();
    let (done_tx, done_rx) = mpsc::channel();
    let leader_done = done_tx.clone();
    let leader = thread::spawn(move || {
        let mut store = Store::default();
        let mut group = group(&mut store, leader_barrier, leader_op);
        let env = FunctionEnv::new(&mut store, leader_env);
        for index in 1..=16 {
            let topology = leader_shared
                .acquire_topology_token(&mut group, &mut store, &env)
                .unwrap();
            let (topology, state) = leader_shared
                .write_linker_state_holding_topology(topology, env.as_ref(&store))
                .unwrap();
            group
                .indirect_function_table
                .grow(&mut store, 1, Value::FuncRef(None))
                .unwrap();
            leader_shared
                .synchronize_link_operation(
                    topology,
                    DlOperation::AllocateFunctionTable { index, size: 1 },
                    state,
                    &mut group,
                    env.as_ref(&store),
                )
                .unwrap();
        }
        leader_done.send(()).unwrap();
    });
    let follower = thread::spawn(move || {
        let mut store = Store::default();
        let mut group = group(&mut store, follower_barrier, follower_op);
        let env = FunctionEnv::new(&mut store, follower_env);
        for size in 2..=17 {
            while !follower_shared.dl_operation_pending_load(Ordering::SeqCst) {
                follower_shared.check_active(env.as_ref(&store)).unwrap();
                thread::yield_now();
            }
            follower_shared
                .do_pending_link_operations_internal(&mut group, &mut store, &env)
                .unwrap();
            assert_eq!(group.indirect_function_table.size(&store), size);
        }
        done_tx.send(()).unwrap();
    });
    for _ in 0..2 {
        if done_rx.recv_timeout(Duration::from_secs(10)).is_err() {
            let _ = leader_process.force_terminate(137.into());
            let _ = follower_process.force_terminate(137.into());
            let _ = leader.join();
            let _ = follower.join();
            panic!("healthy replay after teardown did not finish");
        }
    }
    leader.join().unwrap();
    follower.join().unwrap();
    shared.check_active(&env(&store)).unwrap();
}

#[derive(Debug)]
struct PendingLibrary {
    entered: Option<mpsc::Sender<()>>,
    release: tokio::sync::oneshot::Receiver<()>,
}

impl virtual_fs::AsyncRead for PendingLibrary {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        _buf: &mut virtual_fs::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use std::future::Future;
        if let Some(entered) = self.entered.take() {
            let _ = entered.send(());
        }
        std::pin::Pin::new(&mut self.release)
            .poll(cx)
            .map(|_| Ok(()))
    }
}
impl virtual_fs::AsyncWrite for PendingLibrary {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        _: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::task::Poll::Ready(Err(std::io::ErrorKind::PermissionDenied.into()))
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}
impl virtual_fs::AsyncSeek for PendingLibrary {
    fn start_seek(self: std::pin::Pin<&mut Self>, _: std::io::SeekFrom) -> std::io::Result<()> {
        Ok(())
    }
    fn poll_complete(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<u64>> {
        std::task::Poll::Ready(Ok(0))
    }
}
impl virtual_fs::VirtualFile for PendingLibrary {
    fn last_accessed(&self) -> u64 {
        0
    }
    fn last_modified(&self) -> u64 {
        0
    }
    fn created_time(&self) -> u64 {
        0
    }
    fn size(&self) -> u64 {
        0
    }
    fn set_len(&mut self, _: u64) -> Result<(), virtual_fs::FsError> {
        Err(virtual_fs::FsError::PermissionDenied)
    }
    fn unlink(&mut self) -> Result<(), virtual_fs::FsError> {
        Ok(())
    }
    fn poll_read_ready(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::task::Poll::Ready(Ok(1))
    }
    fn poll_write_ready(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::task::Poll::Ready(Ok(0))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_terminate_cancels_pending_library_read_and_releases_topology() {
    use crate::state::linker::{DlModuleSpec, InProgressLinkState};
    use std::path::Path;
    let store = Store::default();
    let shared = fixture(&store);
    let fs = Arc::new(virtual_fs::mem_fs::FileSystem::default());
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    fs.insert_device_file(
        "/pending.so".into(),
        Box::new(PendingLibrary {
            entered: Some(entered_tx),
            release: release_rx,
        }),
    )
    .unwrap();
    let env = WasiEnv::builder("pending-library-read")
        .engine(store.engine().clone())
        .fs(fs as Arc<dyn virtual_fs::FileSystem + Send + Sync>)
        .build()
        .unwrap();
    let process = env.process.clone();
    let topology = shared.topology_coordinator.try_acquire().unwrap();
    let worker_shared = shared.clone();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let (topology, mut state) = worker_shared
            .write_linker_state_holding_topology(topology, &env)
            .unwrap();
        let result = state.load_module_tree(
            DlModuleSpec::FileSystem {
                module_spec: Path::new("/pending.so"),
                ld_library_path: &[],
            },
            &mut InProgressLinkState::default(),
            &env,
            worker_shared.cancellation(),
            &[] as &[String],
            Option::<&Path>::None,
        );
        drop(state);
        drop(topology);
        let _ = done_tx.send(result);
    });
    entered_rx
        .recv_timeout(DEADLINE)
        .expect("loader never polled its pending file");
    process.force_terminate(137.into()).unwrap();
    let result = done_rx.recv_timeout(DEADLINE);
    if result.is_err() {
        let _ = release_tx.send(());
        let _ = done_rx.recv_timeout(DEADLINE);
    }
    worker.join().unwrap();
    assert_aborted(result.expect("terminated loader retained its native thread"));
    assert!(
        shared.linker_state.try_write().is_ok(),
        "cancelled loader retained its write lock"
    );
    assert!(
        shared.topology_coordinator.try_acquire().is_some(),
        "cancelled loader retained topology"
    );
}
