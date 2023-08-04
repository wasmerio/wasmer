use std::{
    cell::RefCell,
    fmt::Debug,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use bytes::Bytes;
use derivative::*;
use js_sys::{Array, Promise, Uint8Array};
use once_cell::sync::OnceCell;
use tokio::{select, sync::mpsc};
use wasm_bindgen::{prelude::*, JsCast};
use wasmer::AsStoreRef;
use web_sys::{DedicatedWorkerGlobalScope, MessageEvent, Url, Worker, WorkerOptions, WorkerType};

use crate::{
    runtime::{
        module_cache::WebWorkerModuleCache,
        task_manager::{
            InlineWaker, TaskExecModule, TaskWasm, TaskWasmRun, TaskWasmRunProperties,
            WasmResumeTrigger,
        },
        SpawnMemoryType,
    },
    types::wasi::ExitCode,
    utils::web::js_error,
    wasmer::{AsJs, Memory, MemoryType, Module, Store},
    InstanceSnapshot, WasiEnv, WasiFunctionEnv, WasiThreadError,
};

type BoxRun<'a> = Box<dyn FnOnce() + Send + 'a>;

type BoxRunAsync<'a, T> =
    Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = T> + 'static>> + Send + 'a>;

#[derive(Debug, Clone)]
pub(crate) enum WasmMemoryType {
    CreateMemory,
    CreateMemoryOfType(MemoryType),
    ShareMemory(MemoryType),
}

#[derive(Derivative)]
#[derivative(Debug)]
pub(crate) struct WasmRunTrigger {
    #[derivative(Debug = "ignore")]
    run: Box<WasmResumeTrigger>,
    memory_ty: MemoryType,
    env: WasiEnv,
}

#[derive(Derivative)]
#[derivative(Debug)]
pub(crate) enum WebRunCommand {
    ExecModule {
        #[derivative(Debug = "ignore")]
        run: Box<TaskExecModule>,
        module_bytes: Bytes,
    },
    SpawnWasm {
        #[derivative(Debug = "ignore")]
        run: Box<TaskWasmRun>,
        run_type: WasmMemoryType,
        env: WasiEnv,
        module_bytes: Bytes,
        snapshot: Option<InstanceSnapshot>,
        trigger: Option<WasmRunTrigger>,
        update_layout: bool,
        result: Option<Result<Bytes, ExitCode>>,
        pool: WebThreadPool,
    },
}

trait AssertSendSync: Send + Sync {}
impl AssertSendSync for WebThreadPool {}

#[wasm_bindgen]
#[derive(Debug)]
struct WebThreadPoolInner {
    pool_reactors: Arc<PoolStateAsync>,
    pool_dedicated: Arc<PoolStateSync>,
}

#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct WebThreadPool {
    inner: Arc<WebThreadPoolInner>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PoolType {
    Shared,
    Dedicated,
}

#[derive(Derivative)]
#[derivative(Debug)]
struct IdleThreadAsync {
    idx: usize,
    #[derivative(Debug = "ignore")]
    work: mpsc::UnboundedSender<BoxRunAsync<'static, ()>>,
}

impl IdleThreadAsync {
    #[allow(dead_code)]
    fn consume(self, task: BoxRunAsync<'static, ()>) {
        self.work.send(task).unwrap();
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
struct IdleThreadSync {
    idx: usize,
    #[derivative(Debug = "ignore")]
    work: std::sync::mpsc::Sender<BoxRun<'static>>,
}

impl IdleThreadSync {
    #[allow(dead_code)]
    fn consume(self, task: BoxRun<'static>) {
        self.work.send(task).unwrap();
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
struct PoolStateSync {
    #[derivative(Debug = "ignore")]
    idle_rx: Mutex<mpsc::UnboundedReceiver<IdleThreadSync>>,
    idle_tx: mpsc::UnboundedSender<IdleThreadSync>,
    idx_seed: AtomicUsize,
    idle_size: usize,
    blocking: bool,
    spawn: mpsc::UnboundedSender<BoxRun<'static>>,
    #[allow(dead_code)]
    type_: PoolType,
}

#[derive(Derivative)]
#[derivative(Debug)]
struct PoolStateAsync {
    #[derivative(Debug = "ignore")]
    idle_rx: Mutex<mpsc::UnboundedReceiver<IdleThreadAsync>>,
    idle_tx: mpsc::UnboundedSender<IdleThreadAsync>,
    idx_seed: AtomicUsize,
    idle_size: usize,
    blocking: bool,
    spawn: mpsc::UnboundedSender<BoxRunAsync<'static, ()>>,
    #[allow(dead_code)]
    type_: PoolType,
}

enum ThreadState {
    Sync(Arc<ThreadStateSync>),
    Async(Arc<ThreadStateAsync>),
}

struct ThreadStateSync {
    pool: Arc<PoolStateSync>,
    #[allow(dead_code)]
    idx: usize,
    tx: std::sync::mpsc::Sender<BoxRun<'static>>,
    rx: Mutex<Option<std::sync::mpsc::Receiver<BoxRun<'static>>>>,
    init: Mutex<Option<BoxRun<'static>>>,
}

struct ThreadStateAsync {
    pool: Arc<PoolStateAsync>,
    #[allow(dead_code)]
    idx: usize,
    tx: mpsc::UnboundedSender<BoxRunAsync<'static, ()>>,
    rx: Mutex<Option<mpsc::UnboundedReceiver<BoxRunAsync<'static, ()>>>>,
    init: Mutex<Option<BoxRunAsync<'static, ()>>>,
}

fn copy_memory(memory: JsValue, ty: MemoryType) -> Result<JsValue, WasiThreadError> {
    let memory_js = memory.dyn_into::<js_sys::WebAssembly::Memory>().unwrap();

    let descriptor = js_sys::Object::new();

    // Annotation is here to prevent spurious IDE warnings.
    #[allow(unused_unsafe)]
    unsafe {
        js_sys::Reflect::set(&descriptor, &"initial".into(), &ty.minimum.0.into()).unwrap();
        if let Some(max) = ty.maximum {
            js_sys::Reflect::set(&descriptor, &"maximum".into(), &max.0.into()).unwrap();
        }
        js_sys::Reflect::set(&descriptor, &"shared".into(), &ty.shared.into()).unwrap();
    }

    let new_memory = js_sys::WebAssembly::Memory::new(&descriptor).map_err(|_e| {
        WasiThreadError::MemoryCreateFailed(wasmer::MemoryError::Generic(
            "Error while creating the memory".to_owned(),
        ))
    })?;

    let src_buffer = memory_js.buffer();
    let src_size: u64 = src_buffer
        .unchecked_ref::<js_sys::ArrayBuffer>()
        .byte_length()
        .into();
    let src_view = js_sys::Uint8Array::new(&src_buffer);

    let pages = ((src_size as usize - 1) / wasmer::WASM_PAGE_SIZE) + 1;
    new_memory.grow(pages as u32);

    let dst_buffer = new_memory.buffer();
    let dst_view = js_sys::Uint8Array::new(&dst_buffer);

    tracing::trace!(%src_size, "memory copy started");

    {
        let mut offset = 0;
        let mut chunk = [0u8; 40960];
        while offset < src_size {
            let remaining = src_size - offset;
            let sublen = remaining.min(chunk.len() as u64) as usize;
            let end = offset.checked_add(sublen.try_into().unwrap()).unwrap();
            src_view
                .subarray(offset.try_into().unwrap(), end.try_into().unwrap())
                .copy_to(&mut chunk[..sublen]);
            dst_view
                .subarray(offset.try_into().unwrap(), end.try_into().unwrap())
                .copy_from(&chunk[..sublen]);
            offset += sublen as u64;
        }
    }

    Ok(new_memory.into())
}

impl WebThreadPool {
    pub fn new(size: usize) -> WebThreadPool {
        tracing::info!(size, "pool created");

        let (idle_tx_shared, idle_rx_shared) = mpsc::unbounded_channel();
        let (idle_tx_dedicated, idle_rx_dedicated) = mpsc::unbounded_channel();

        let (spawn_tx_shared, mut spawn_rx_shared) = mpsc::unbounded_channel();
        let (spawn_tx_dedicated, mut spawn_rx_dedicated) = mpsc::unbounded_channel();

        let pool_reactors = PoolStateAsync {
            idle_rx: Mutex::new(idle_rx_shared),
            idle_tx: idle_tx_shared,
            idx_seed: AtomicUsize::new(0),
            blocking: false,
            idle_size: 2usize.max(size),
            type_: PoolType::Shared,
            spawn: spawn_tx_shared,
        };

        let pool_dedicated = PoolStateSync {
            idle_rx: Mutex::new(idle_rx_dedicated),
            idle_tx: idle_tx_dedicated,
            idx_seed: AtomicUsize::new(0),
            blocking: true,
            idle_size: 1usize.max(size),
            type_: PoolType::Dedicated,
            spawn: spawn_tx_dedicated,
        };

        let inner = Arc::new(WebThreadPoolInner {
            pool_dedicated: Arc::new(pool_dedicated),
            pool_reactors: Arc::new(pool_reactors),
        });

        let inner1 = inner.clone();
        let inner3 = inner.clone();

        // The management thread will spawn other threads - this thread is safe from
        // being blocked by other threads
        wasm_bindgen_futures::spawn_local(async move {
            loop {
                select! {
                    spawn = spawn_rx_shared.recv() => {
                        if let Some(spawn) = spawn { inner1.pool_reactors.expand(spawn); } else { break; }
                    }
                    spawn = spawn_rx_dedicated.recv() => {
                        if let Some(spawn) = spawn { inner3.pool_dedicated.expand(spawn); } else { break; }
                    }
                }
            }
        });

        WebThreadPool { inner }
    }

    pub fn new_with_max_threads() -> Result<WebThreadPool, anyhow::Error> {
        let global = js_sys::global();

        let hardware_concurrency = if let Some(window) = global.dyn_ref::<web_sys::Window>() {
            window.navigator().hardware_concurrency()
        } else if let Some(worker_scope) = global.dyn_ref::<web_sys::DedicatedWorkerGlobalScope>() {
            worker_scope.navigator().hardware_concurrency()
        } else {
            anyhow::bail!("Unable to determine the available concurrency");
        };

        let hardware_concurrency = hardware_concurrency as usize;
        let pool_size = std::cmp::max(hardware_concurrency, 1);

        Ok(WebThreadPool::new(pool_size))
    }

    pub fn spawn_shared(&self, task: BoxRunAsync<'static, ()>) {
        self.inner.pool_reactors.spawn(task);
    }

    pub fn spawn_wasm(&self, task: TaskWasm) -> Result<(), WasiThreadError> {
        let run = task.run;
        let env = task.env;
        let module = task.module;
        let module_bytes = module.serialize().unwrap();
        let snapshot = task.snapshot.cloned();
        let trigger = task.trigger;
        let update_layout = task.update_layout;

        let mut memory_ty = None;
        let mut memory = JsValue::null();
        let run_type = match task.spawn_type {
            SpawnMemoryType::CreateMemory => WasmMemoryType::CreateMemory,
            SpawnMemoryType::CreateMemoryOfType(ty) => {
                memory_ty = Some(ty);
                WasmMemoryType::CreateMemoryOfType(ty)
            }
            SpawnMemoryType::CopyMemory(m, store) => {
                memory_ty = Some(m.ty(&store));
                memory = m.as_jsvalue(&store);

                // We copy the memory here rather than later as
                // the fork syscalls need to copy the memory
                // synchronously before the next thread accesses
                // and before the fork parent resumes, otherwise
                // there will be memory corruption
                memory = copy_memory(memory, m.ty(&store))?;

                WasmMemoryType::ShareMemory(m.ty(&store))
            }
            SpawnMemoryType::ShareMemory(m, store) => {
                memory_ty = Some(m.ty(&store));
                memory = m.as_jsvalue(&store);
                WasmMemoryType::ShareMemory(m.ty(&store))
            }
        };

        let task = Box::new(WebRunCommand::SpawnWasm {
            trigger: trigger.map(|trigger| WasmRunTrigger {
                run: trigger,
                memory_ty: memory_ty.expect("triggers must have the a known memory type"),
                env: env.clone(),
            }),
            run,
            run_type,
            env,
            module_bytes,
            snapshot,
            update_layout,
            result: None,
            pool: self.clone(),
        });
        let task = Box::into_raw(task);

        let module = JsValue::from(module)
            .dyn_into::<js_sys::WebAssembly::Module>()
            .unwrap();
        schedule_task(JsValue::from(task as u32), module, memory);
        Ok(())
    }

    pub fn spawn_dedicated(&self, task: BoxRun<'static>) {
        self.inner.pool_dedicated.spawn(task);
    }
}

fn _build_ctx_and_store(
    module: js_sys::WebAssembly::Module,
    memory: JsValue,
    module_bytes: Bytes,
    env: WasiEnv,
    run_type: WasmMemoryType,
    snapshot: Option<InstanceSnapshot>,
    update_layout: bool,
) -> Option<(WasiFunctionEnv, Store)> {
    // Compile the web assembly module
    let module: Module = (module, module_bytes).into();

    // Make a fake store which will hold the memory we just transferred
    let mut temp_store = env.runtime().new_store();
    let spawn_type = match run_type {
        WasmMemoryType::CreateMemory => SpawnMemoryType::CreateMemory,
        WasmMemoryType::CreateMemoryOfType(mem) => SpawnMemoryType::CreateMemoryOfType(mem),
        WasmMemoryType::ShareMemory(ty) => {
            let memory = match Memory::from_jsvalue(&mut temp_store, &ty, &memory) {
                Ok(a) => a,
                Err(e) => {
                    let err = js_error(e.into());
                    tracing::error!(error = &*err, "Failed to receive memory for module");
                    return None;
                }
            };
            SpawnMemoryType::ShareMemory(memory, temp_store.as_store_ref())
        }
    };

    let snapshot = snapshot.as_ref();
    let (ctx, store) =
        match WasiFunctionEnv::new_with_store(module, env, snapshot, spawn_type, update_layout) {
            Ok(a) => a,
            Err(err) => {
                tracing::error!(
                    error = &err as &dyn std::error::Error,
                    "Failed to crate wasi context",
                );
                return None;
            }
        };
    Some((ctx, store))
}

async fn _compile_module(bytes: &[u8]) -> Result<js_sys::WebAssembly::Module, anyhow::Error> {
    let js_bytes = unsafe { Uint8Array::view(bytes) };
    Ok(
        match wasm_bindgen_futures::JsFuture::from(js_sys::WebAssembly::compile(&js_bytes.into()))
            .await
        {
            Ok(a) => match a.dyn_into::<js_sys::WebAssembly::Module>() {
                Ok(a) => a,
                Err(err) => {
                    return Err(anyhow::format_err!(
                        "Failed to compile module - {}",
                        err.as_string().unwrap_or_else(|| format!("{:?}", err))
                    ));
                }
            },
            Err(err) => {
                return Err(anyhow::format_err!(
                    "WebAssembly failed to compile - {}",
                    err.as_string().unwrap_or_else(|| format!("{:?}", err))
                ));
            }
        }, //js_sys::WebAssembly::Module::new(&js_bytes.into()).unwrap()
    )
}

impl PoolStateAsync {
    fn spawn(&self, task: BoxRunAsync<'static, ()>) {
        for _ in 0..10 {
            if let Ok(mut guard) = self.idle_rx.try_lock() {
                if let Ok(thread) = guard.try_recv() {
                    thread.consume(task);
                    return;
                }
                break;
            }
            std::thread::yield_now();
        }

        self.spawn.send(task).unwrap();
    }

    fn expand(self: &Arc<Self>, init: BoxRunAsync<'static, ()>) {
        let idx = self.idx_seed.fetch_add(1usize, Ordering::Release);

        let (tx, rx) = mpsc::unbounded_channel();

        let state_inner = Arc::new(ThreadStateAsync {
            pool: Arc::clone(self),
            idx,
            tx,
            rx: Mutex::new(Some(rx)),
            init: Mutex::new(Some(init)),
        });
        let state = Arc::new(ThreadState::Async(state_inner.clone()));
        start_worker_now(idx, state, state_inner.pool.type_ /* , None */);
    }
}

impl PoolStateSync {
    fn spawn(&self, task: BoxRun<'static>) {
        for _ in 0..10 {
            if let Ok(mut guard) = self.idle_rx.try_lock() {
                if let Ok(thread) = guard.try_recv() {
                    thread.consume(task);
                    return;
                }
                break;
            }
            std::thread::yield_now();
        }

        self.spawn.send(task).unwrap();
    }

    fn expand(self: &Arc<Self>, init: BoxRun<'static>) {
        let idx = self.idx_seed.fetch_add(1usize, Ordering::Release);

        let (tx, rx) = std::sync::mpsc::channel();

        let state_inner = Arc::new(ThreadStateSync {
            pool: Arc::clone(self),
            idx,
            tx,
            rx: Mutex::new(Some(rx)),
            init: Mutex::new(Some(init)),
        });
        let state = Arc::new(ThreadState::Sync(state_inner.clone()));
        start_worker_now(idx, state, state_inner.pool.type_ /* , None */);
    }
}

fn start_worker_now(
    idx: usize,
    state: Arc<ThreadState>,
    type_: PoolType,
    /* should_warn_on_error: Option<Terminal>, */
) {
    let mut opts = WorkerOptions::new();
    opts.type_(WorkerType::Module);
    opts.name(&format!("Worker-{:?}-{}", type_, idx));

    let ptr = Arc::into_raw(state);

    let result = start_worker(
        wasm_bindgen::module()
            .dyn_into::<js_sys::WebAssembly::Module>()
            .unwrap(),
        wasm_bindgen::memory(),
        JsValue::from(ptr as u32),
        opts,
    );
    _process_worker_result(result);
}

fn _process_worker_result(result: Result<(), JsValue>) {
    if let Err(err) = result {
        let err = js_error(err);
        tracing::error!(error = &*err, "failed to start worker thread");
    }
}

impl ThreadStateSync {
    fn work(state: Arc<ThreadStateSync>) {
        let thread_index = state.idx;

        let _span = tracing::info_span!("dedicated_worker",
                thread.index=thread_index,
                thread_type_=?state.pool.type_,
        )
        .entered();

        // Load the work queue receiver where other people will
        // send us the work that needs to be done
        let work_rx = {
            let mut lock = state.rx.lock().unwrap();
            lock.take().unwrap()
        };

        // Load the initial work
        let mut work = {
            let mut lock = state.init.lock().unwrap();
            lock.take()
        };

        // The work is done in an asynchronous engine (that supports Javascript)
        let work_tx = state.tx.clone();
        let pool = Arc::clone(&state.pool);
        let global = js_sys::global().unchecked_into::<DedicatedWorkerGlobalScope>();

        loop {
            // Process work until we need to go idle
            while let Some(task) = work {
                task();

                // Grab the next work
                work = work_rx.try_recv().ok();
            }

            // If there iss already an idle thread thats older then
            // keep that one (otherwise ditch it) - this creates negative
            // pressure on the pool size.
            // The reason we keep older threads is to maximize cache hits such
            // as module compile caches.
            if let Ok(mut lock) = state.pool.idle_rx.try_lock() {
                let mut others = Vec::new();
                while let Ok(other) = lock.try_recv() {
                    others.push(other);
                }

                // Sort them in the order of index (so older ones come first)
                others.sort_by_key(|k| k.idx);

                // If the number of others (plus us) exceeds the maximum then
                // we either drop ourselves or one of the others
                if others.len() + 1 > pool.idle_size {
                    // How many are there already there that have a lower index - are we the one without a chair?
                    let existing = others
                        .iter()
                        .map(|a| a.idx)
                        .filter(|a| *a < thread_index)
                        .count();
                    if existing >= pool.idle_size {
                        for other in others {
                            state.pool.idle_tx.send(other).unwrap();
                        }
                        tracing::info!("worker closed");
                        break;
                    } else {
                        // Someone else is the one (the last one)
                        let leftover_chairs = others.len() - 1;
                        for other in others.into_iter().take(leftover_chairs) {
                            state.pool.idle_tx.send(other).unwrap();
                        }
                    }
                } else {
                    // Add them all back in again (but in the right order)
                    for other in others {
                        state.pool.idle_tx.send(other).unwrap();
                    }
                }
            }
            let idle = IdleThreadSync {
                idx: thread_index,
                work: work_tx.clone(),
            };
            if let Err(_) = state.pool.idle_tx.send(idle) {
                tracing::info!("pool is closed");
                break;
            }

            // Do a blocking recv (if this fails the thread is closed)
            work = match work_rx.recv() {
                Ok(a) => Some(a),
                Err(err) => {
                    tracing::info!(error = &err as &dyn std::error::Error, "worker closed");
                    break;
                }
            };
        }

        global.close();
    }
}

impl ThreadStateAsync {
    fn work(state: Arc<ThreadStateAsync>) {
        let thread_index = state.idx;
        let _span = tracing::info_span!("shared_worker",
                thread.index=thread_index,
                thread_type_=?state.pool.type_,
        )
        .entered();

        // Load the work queue receiver where other people will
        // send us the work that needs to be done
        let mut work_rx = {
            let mut lock = state.rx.lock().unwrap();
            lock.take().unwrap()
        };

        // Load the initial work
        let mut work = {
            let mut lock = state.init.lock().unwrap();
            lock.take()
        };

        // The work is done in an asynchronous engine (that supports Javascript)
        let work_tx = state.tx.clone();
        let pool = Arc::clone(&state.pool);
        let driver = async move {
            let global = js_sys::global().unchecked_into::<DedicatedWorkerGlobalScope>();

            loop {
                // Process work until we need to go idle
                while let Some(task) = work {
                    let future = task();
                    if pool.blocking {
                        future.await;
                    } else {
                        wasm_bindgen_futures::spawn_local(async move {
                            future.await;
                        });
                    }

                    // Grab the next work
                    work = work_rx.try_recv().ok();
                }

                // If there iss already an idle thread thats older then
                // keep that one (otherwise ditch it) - this creates negative
                // pressure on the pool size.
                // The reason we keep older threads is to maximize cache hits such
                // as module compile caches.
                if let Ok(mut lock) = state.pool.idle_rx.try_lock() {
                    let mut others = Vec::new();
                    while let Ok(other) = lock.try_recv() {
                        others.push(other);
                    }

                    // Sort them in the order of index (so older ones come first)
                    others.sort_by_key(|k| k.idx);

                    // If the number of others (plus us) exceeds the maximum then
                    // we either drop ourselves or one of the others
                    if others.len() + 1 > pool.idle_size {
                        // How many are there already there that have a lower index - are we the one without a chair?
                        let existing = others
                            .iter()
                            .map(|a| a.idx)
                            .filter(|a| *a < thread_index)
                            .count();
                        if existing >= pool.idle_size {
                            for other in others {
                                state.pool.idle_tx.send(other).unwrap();
                            }
                            tracing::info!("worker closed");
                            break;
                        } else {
                            // Someone else is the one (the last one)
                            let leftover_chairs = others.len() - 1;
                            for other in others.into_iter().take(leftover_chairs) {
                                state.pool.idle_tx.send(other).unwrap();
                            }
                        }
                    } else {
                        // Add them all back in again (but in the right order)
                        for other in others {
                            state.pool.idle_tx.send(other).unwrap();
                        }
                    }
                }

                // Now register ourselves as idle
                /*
                trace!(
                    "pool is idle (thread_index={}, type={:?})",
                    thread_index,
                    pool.type_
                );
                */
                let idle = IdleThreadAsync {
                    idx: thread_index,
                    work: work_tx.clone(),
                };
                if let Err(_) = state.pool.idle_tx.send(idle) {
                    tracing::info!("pool is closed");
                    break;
                }

                // Do a blocking recv (if this fails the thread is closed)
                work = match work_rx.recv().await {
                    Some(a) => Some(a),
                    None => {
                        tracing::info!("worker closed");
                        break;
                    }
                };
            }

            global.close();
        };
        wasm_bindgen_futures::spawn_local(driver);
    }
}

#[wasm_bindgen(skip_typescript)]
pub fn worker_entry_point(state_ptr: u32) {
    let state = unsafe { Arc::<ThreadState>::from_raw(state_ptr as *const ThreadState) };

    let name = js_sys::global()
        .unchecked_into::<DedicatedWorkerGlobalScope>()
        .name();
    tracing::debug!(%name, "Entry");

    match state.as_ref() {
        ThreadState::Async(state) => {
            ThreadStateAsync::work(state.clone());
        }
        ThreadState::Sync(state) => {
            ThreadStateSync::work(state.clone());
        }
    }
}

#[wasm_bindgen(skip_typescript)]
pub fn wasm_entry_point(
    task_ptr: u32,
    wasm_module: js_sys::WebAssembly::Module,
    wasm_memory: JsValue,
    wasm_cache: JsValue,
) {
    // Import the WASM cache
    WebWorkerModuleCache::import(wasm_cache);

    // Grab the run wrapper that passes us the rust variables (and extract the callback)
    let task = task_ptr as *mut WebRunCommand;
    let task = unsafe { Box::from_raw(task) };
    match *task {
        WebRunCommand::ExecModule { run, module_bytes } => {
            let module: Module = (wasm_module, module_bytes).into();
            run(module);
        }
        WebRunCommand::SpawnWasm {
            run,
            run_type,
            env,
            module_bytes,
            snapshot,
            mut trigger,
            update_layout,
            mut result,
            ..
        } => {
            // If there is a trigger then run it
            let trigger = trigger.take();
            if let Some(trigger) = trigger {
                let trigger_run = trigger.run;
                result = Some(InlineWaker::block_on(trigger_run()));
            }

            // Invoke the callback which will run the web assembly module
            if let Some((ctx, store)) = _build_ctx_and_store(
                wasm_module,
                wasm_memory,
                module_bytes,
                env,
                run_type,
                snapshot,
                update_layout,
            ) {
                run(TaskWasmRunProperties {
                    ctx,
                    store,
                    trigger_result: result,
                });
            };
        }
    }
}

struct WebWorker {
    worker: Worker,
    available: bool,
}

std::thread_local! {
    static WEB_WORKER_POOL: RefCell<Vec<WebWorker>>
        = RefCell::new(Vec::new());
}

fn register_web_worker(web_worker: Worker) -> usize {
    WEB_WORKER_POOL.with(|u| {
        let mut workers = u.borrow_mut();
        workers.push(WebWorker {
            worker: web_worker,
            available: false,
        });
        workers.len() - 1
    })
}

fn return_web_worker(id: usize) {
    WEB_WORKER_POOL.with(|u| {
        let mut workers = u.borrow_mut();
        let worker = workers.get_mut(id);
        if let Some(worker) = worker {
            worker.available = true;
        }
    });
}

fn get_web_worker(id: usize) -> Option<Worker> {
    WEB_WORKER_POOL.with(|u| {
        let workers = u.borrow();
        workers.get(id).map(|worker| worker.worker.clone())
    })
}

fn claim_web_worker() -> Option<usize> {
    WEB_WORKER_POOL.with(|u| {
        let mut workers = u.borrow_mut();
        for (n, worker) in workers.iter_mut().enumerate() {
            if worker.available {
                worker.available = false;
                return Some(n);
            }
        }
        None
    })
}

async fn schedule_wasm_task(
    task_ptr: u32,
    wasm_module: js_sys::WebAssembly::Module,
    wasm_memory: JsValue,
) {
    // Grab the run wrapper that passes us the rust variables
    let task = task_ptr as *mut WebRunCommand;
    let task = unsafe { Box::from_raw(task) };
    match *task {
        WebRunCommand::ExecModule { run, module_bytes } => {
            let module: Module = (wasm_module, module_bytes).into();
            run(module);
        }
        WebRunCommand::SpawnWasm {
            run,
            run_type,
            env,
            module_bytes,
            snapshot,
            mut trigger,
            update_layout,
            mut result,
            pool,
        } => {
            // We will pass it on now
            let trigger = trigger.take();
            let trigger_rx = if let Some(trigger) = trigger {
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

                // We execute the trigger on another thread as any atomic operations (such as wait)
                // are not allowed on the main thread and even through the tokio is asynchronous that
                // does not mean it does not have short synchronous blocking events (which it does)
                pool.spawn_shared(Box::new(|| {
                    Box::pin(async move {
                        let run = trigger.run;
                        let ret = run().await;
                        tx.send(ret).ok();
                    })
                }));
                Some(rx)
            } else {
                None
            };

            // Export the cache
            let wasm_cache = WebWorkerModuleCache::export();

            // We will now spawn the process in its own thread
            let mut opts = WorkerOptions::new();
            opts.type_(WorkerType::Module);
            opts.name("Wasm-Thread");

            if let Some(mut trigger_rx) = trigger_rx {
                result = trigger_rx.recv().await;
            }

            let task = Box::new(WebRunCommand::SpawnWasm {
                run,
                run_type,
                env,
                module_bytes,
                snapshot,
                trigger: None,
                update_layout,
                result,
                pool,
            });
            let task = Box::into_raw(task);
            let result = start_wasm(
                wasm_bindgen::module()
                    .dyn_into::<js_sys::WebAssembly::Module>()
                    .unwrap(),
                wasm_bindgen::memory(),
                JsValue::from(task as u32),
                opts,
                wasm_module,
                wasm_memory,
                wasm_cache,
            );
            _process_worker_result(result /* , None */);
        }
    }
}

fn new_worker(opts: &WorkerOptions) -> Result<Worker, JsValue> {
    static WORKER_URL: OnceCell<Box<str>> = OnceCell::new();
    fn init_worker_url() -> Result<Box<str>, JsValue> {
        #[wasm_bindgen]
        extern "C" {
            #[wasm_bindgen(js_namespace = ["import", "meta"], js_name = url)]
            static IMPORT_META_URL: String;
        }
        Ok(Url::create_object_url_with_blob(
            &web_sys::Blob::new_with_u8_array_sequence_and_options(
                Array::from_iter([Uint8Array::from(format!(
r#"Error.stackTraceLimit=50;globalThis.onmessage=async ev=>{{
    if(ev.data.length==4){{
        let[module,memory,state]=ev.data;
        const{{default:init,worker_entry_point}}=await import({0});
        await init(module,memory);
        worker_entry_point(state);
    }}else{{
        var is_returned=false;
        try{{
            globalThis.onmessage=ev=>{{console.error("wasm threads can only run a single process then exit",ev)}}
            let[id,module,memory,ctx,wasm_module,wasm_memory,wasm_cache]=ev.data;
            const{{default:init,wasm_entry_point}}=await import({0});
            await init(module,memory);
            wasm_entry_point(ctx,wasm_module,wasm_memory,wasm_cache);
            // Return the web worker to the thread pool
            postMessage([id]);
            is_returned=true;
        }}finally{{//Terminate the worker
            if(is_returned==false){{close();}}
        }}
    }}
}}"#
                , IMPORT_META_URL.clone()).as_bytes())]).as_ref(),
                web_sys::BlobPropertyBag::new().type_("application/javascript"),
            )?,
        )?
        .into_boxed_str())
    }
    match WORKER_URL.get() {
        Some(script_url) => Worker::new_with_options(script_url, opts),
        None => {
            if let Err(e) = WORKER_URL.set(init_worker_url()?) {
                let _ = Url::revoke_object_url(&e);
            };
            Worker::new_with_options(
                WORKER_URL
                    .get()
                    .ok_or_else(|| JsValue::from_str("Worker Blob could not be obtained"))?,
                opts,
            )
        }
    }
}

fn start_worker(
    module: js_sys::WebAssembly::Module,
    memory: JsValue,
    shared_data: JsValue,
    opts: WorkerOptions,
) -> Result<(), JsValue> {
    fn onmessage(event: MessageEvent) -> Promise {
        let data = event.data().unchecked_into::<Array>();
        let task = data.get(0).unchecked_into_f64() as u32;
        let module = data.get(1).dyn_into().unwrap();
        let memory = data.get(2);
        wasm_bindgen_futures::future_to_promise(async move {
            schedule_wasm_task(task, module, memory).await;
            Ok(JsValue::UNDEFINED)
        })
    }
    let worker = new_worker(&opts)?;
    worker.set_onmessage(Some(
        Closure::<dyn Fn(MessageEvent) -> Promise + 'static>::new(onmessage)
            .as_ref()
            .unchecked_ref(),
    ));
    worker.post_message(Array::from_iter([JsValue::from(module), memory, shared_data]).as_ref())
}
fn start_wasm(
    module: js_sys::WebAssembly::Module,
    memory: JsValue,
    ctx: JsValue,
    opts: WorkerOptions,
    wasm_module: js_sys::WebAssembly::Module,
    wasm_memory: JsValue,
    wasm_cache: JsValue,
) -> Result<(), JsValue> {
    fn onmessage(event: MessageEvent) -> Promise {
        let data = event.data().unchecked_into::<Array>();
        if data.length() == 3 {
            let task = data.get(0).unchecked_into_f64() as u32;
            let module = data.get(1).dyn_into().unwrap();
            let memory = data.get(2);
            wasm_bindgen_futures::future_to_promise(async move {
                schedule_wasm_task(task, module, memory).await;
                Ok(JsValue::UNDEFINED)
            })
        } else {
            let id = data.get(0).unchecked_into_f64() as usize;
            return_web_worker(id);
            Promise::resolve(&JsValue::UNDEFINED)
        }
    }
    let worker;
    let worker_id;
    if let Some(id) = claim_web_worker() {
        worker_id = id;
        worker = get_web_worker(worker_id)
            .ok_or_else(|| JsError::new("failed to retrieve worker from worker pool"))?;
    } else {
        worker = new_worker(&opts)?;
        worker_id = register_web_worker(worker.clone());
    }
    worker.set_onmessage(Some(
        Closure::<dyn Fn(MessageEvent) -> Promise + 'static>::new(onmessage)
            .as_ref()
            .unchecked_ref(),
    ));
    worker.post_message(
        Array::from_iter([
            JsValue::from(worker_id),
            JsValue::from(module),
            memory,
            ctx,
            JsValue::from(wasm_module),
            wasm_memory,
            wasm_cache,
        ])
        .as_ref(),
    )
}

pub(crate) fn schedule_task(task: JsValue, module: js_sys::WebAssembly::Module, memory: JsValue) {
    if let Err(err) = js_sys::global()
        .unchecked_into::<DedicatedWorkerGlobalScope>()
        .post_message(Array::from_iter([task, module.into(), memory]).as_ref())
    {
        let err = js_error(err);
        tracing::error!(error = &*err, "failed to schedule task from worker thread");
    };
}
