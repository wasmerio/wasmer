#![allow(unused_imports)]
use std::{
    borrow::Borrow,
    cell::RefCell,
    collections::{HashMap, VecDeque},
    fmt::Debug,
    future::Future,
    num::NonZeroU32,
    ops::{Deref, DerefMut},
    pin::Pin,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use bytes::Bytes;
use derivative::*;
use js_sys::{JsString, Promise, Uint8Array};
use once_cell::sync::Lazy;
use tokio::{
    select,
    sync::{broadcast, mpsc, oneshot, Semaphore},
};
#[allow(unused_imports, dead_code)]
use tracing::{debug, error, info, trace, warn};
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::JsFuture;
use wasmer::AsStoreRef;
use wasmer_wasix::{
    capture_snapshot,
    runtime::{
        task_manager::{
            InlineWaker, TaskExecModule, TaskWasm, TaskWasmRun, TaskWasmRunProperties,
            WasmResumeTask, WasmResumeTrigger,
        },
        SpawnMemoryType,
    },
    types::wasi::ExitCode,
    wasmer::{AsJs, Memory, MemoryType, Module, Store, WASM_MAX_PAGES},
    wasmer_wasix_types::wasi::Errno,
    InstanceSnapshot, VirtualTaskManager, WasiEnv, WasiFunctionEnv, WasiThreadError,
};
use web_sys::{DedicatedWorkerGlobalScope, WorkerOptions, WorkerType};
use xterm_js_rs::Terminal;

use super::{common::*, interval::*};
use crate::{module_cache::WebWorkerModuleCache, runtime::WebTaskManager};

pub type BoxRun<'a> = Box<dyn FnOnce() + Send + 'a>;

pub type BoxRunAsync<'a, T> =
    Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = T> + 'static>> + Send + 'a>;

#[derive(Debug, Clone)]
pub enum WasmMemoryType {
    CreateMemory,
    CreateMemoryOfType(MemoryType),
    ShareMemory(MemoryType),
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct WasmRunTrigger {
    #[derivative(Debug = "ignore")]
    run: Box<WasmResumeTrigger>,
    memory_ty: MemoryType,
    env: WasiEnv,
}

#[derive(Derivative)]
#[derivative(Debug)]
pub enum WebRunCommand {
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
pub struct WebThreadPoolInner {
    pool_reactors: Arc<PoolStateAsync>,
    pool_dedicated: Arc<PoolStateSync>,
}

#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct WebThreadPool {
    inner: Arc<WebThreadPoolInner>,
}

impl Deref for WebThreadPool {
    type Target = WebThreadPoolInner;

    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PoolType {
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
pub struct PoolStateSync {
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
pub struct PoolStateAsync {
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

pub enum ThreadState {
    Sync(Arc<ThreadStateSync>),
    Async(Arc<ThreadStateAsync>),
}

pub struct ThreadStateSync {
    pool: Arc<PoolStateSync>,
    #[allow(dead_code)]
    idx: usize,
    tx: std::sync::mpsc::Sender<BoxRun<'static>>,
    rx: Mutex<Option<std::sync::mpsc::Receiver<BoxRun<'static>>>>,
    init: Mutex<Option<BoxRun<'static>>>,
}

pub struct ThreadStateAsync {
    pool: Arc<PoolStateAsync>,
    #[allow(dead_code)]
    idx: usize,
    tx: mpsc::UnboundedSender<BoxRunAsync<'static, ()>>,
    rx: Mutex<Option<mpsc::UnboundedReceiver<BoxRunAsync<'static, ()>>>>,
    init: Mutex<Option<BoxRunAsync<'static, ()>>>,
}

#[wasm_bindgen(module = "/public/core.js")]
extern "C" {
    #[wasm_bindgen(js_name = "startWorker")]
    fn start_worker(
        module: js_sys::WebAssembly::Module,
        memory: JsValue,
        shared_data: JsValue,
        opts: WorkerOptions,
    ) -> Promise;

    #[wasm_bindgen(js_name = "startWasm")]
    fn start_wasm(
        module: js_sys::WebAssembly::Module,
        memory: JsValue,
        ctx: JsValue,
        opts: WorkerOptions,
        wasm_module: js_sys::WebAssembly::Module,
        wasm_memory: JsValue,
        wasm_cache: JsValue,
    ) -> Promise;
}

#[wasm_bindgen(module = "/public/worker.js")]
extern "C" {
    #[wasm_bindgen(js_name = "scheduleTask")]
    pub fn schedule_task(task: JsValue, module: js_sys::WebAssembly::Module, memory: JsValue);
}

fn copy_memory(memory: JsValue, ty: MemoryType) -> Result<JsValue, WasiThreadError> {
    let memory_js = memory
        .clone()
        .dyn_into::<js_sys::WebAssembly::Memory>()
        .unwrap();

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

    #[cfg(feature = "tracing")]
    trace!(%src_size, "memory copy started {}");

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
    pub fn new(size: usize) -> Result<WebThreadPool, JsValue> {
        info!("pool::create(size={})", size);

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

        let pool = WebThreadPool { inner };

        Ok(pool)
    }

    pub fn new_with_max_threads() -> Result<WebThreadPool, JsValue> {
        #[wasm_bindgen]
        extern "C" {
            #[wasm_bindgen(js_namespace = navigator, js_name = hardwareConcurrency)]
            static HARDWARE_CONCURRENCY: usize;
        }
        let pool_size = std::cmp::max(*HARDWARE_CONCURRENCY, 1);
        debug!("pool::max_threads={}", pool_size);
        Self::new(pool_size)
    }

    pub fn spawn_shared(&self, task: BoxRunAsync<'static, ()>) {
        self.inner.pool_reactors.spawn(task);
    }

    pub fn spawn_wasm(&self, task: TaskWasm) -> Result<(), WasiThreadError> {
        let run = task.run;
        let env = task.env;
        let module = task.module;
        let module_bytes = module.serialize().unwrap();
        let snapshot = task.snapshot.map(|s| s.clone());
        let trigger = task.trigger;
        let update_layout = task.update_layout;

        let mut memory_ty = None;
        let mut memory = JsValue::null();
        let run_type = match task.spawn_type {
            SpawnMemoryType::CreateMemory => WasmMemoryType::CreateMemory,
            SpawnMemoryType::CreateMemoryOfType(ty) => {
                memory_ty = Some(ty.clone());
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
                Err(_) => {
                    error!("Failed to receive memory for module");
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
                error!("Failed to crate wasi context - {}", err);
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
        start_worker_now(idx, state, state_inner.pool.type_, None);
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
        start_worker_now(idx, state, state_inner.pool.type_, None);
    }
}

pub fn start_worker_now(
    idx: usize,
    state: Arc<ThreadState>,
    type_: PoolType,
    should_warn_on_error: Option<Terminal>,
) {
    let mut opts = WorkerOptions::new();
    opts.type_(WorkerType::Module);
    opts.name(&*format!("Worker-{:?}-{}", type_, idx));

    let ptr = Arc::into_raw(state);

    let result = wasm_bindgen_futures::JsFuture::from(start_worker(
        wasm_bindgen::module()
            .dyn_into::<js_sys::WebAssembly::Module>()
            .unwrap(),
        wasm_bindgen::memory(),
        JsValue::from(ptr as u32),
        opts,
    ));

    wasm_bindgen_futures::spawn_local(async move {
        _process_worker_result(result, should_warn_on_error).await;
    });
}

async fn _process_worker_result(result: JsFuture, should_warn_on_error: Option<Terminal>) {
    let ret = result.await;
    if let Err(err) = ret {
        let err = err.as_string().unwrap_or_else(|| format!("{:?}", err));
        error!("failed to start worker thread - {}", err);

        if let Some(term) = should_warn_on_error {
            term.write(
                wasmer_wasix::os::cconst::ConsoleConst::BAD_WORKER
                    .replace("\n", "\r\n")
                    .replace("\\x1B", "\x1B")
                    .replace("{error}", err.as_str())
                    .as_str(),
            );
        }

        return;
    }
}

impl ThreadStateSync {
    fn work(state: Arc<ThreadStateSync>) {
        let thread_index = state.idx;
        info!(
            "worker dedicated started (index={}, type={:?})",
            thread_index, state.pool.type_
        );

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
                        info!(
                            "worker closed (index={}, type={:?})",
                            thread_index, pool.type_
                        );
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
                info!(
                    "pool is closed (thread_index={}, type={:?})",
                    thread_index, pool.type_
                );
                break;
            }

            // Do a blocking recv (if this fails the thread is closed)
            work = match work_rx.recv() {
                Ok(a) => Some(a),
                Err(err) => {
                    info!(
                        "worker closed (index={}, type={:?}) - {}",
                        thread_index, pool.type_, err
                    );
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
        info!(
            "worker shared started (index={}, type={:?})",
            thread_index, state.pool.type_
        );

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
                            info!(
                                "worker closed (index={}, type={:?})",
                                thread_index, pool.type_
                            );
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
                    info!(
                        "pool is closed (thread_index={}, type={:?})",
                        thread_index, pool.type_
                    );
                    break;
                }

                // Do a blocking recv (if this fails the thread is closed)
                work = match work_rx.recv().await {
                    Some(a) => Some(a),
                    None => {
                        info!(
                            "worker closed (index={}, type={:?})",
                            thread_index, pool.type_
                        );
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
    debug!("{}: Entry", name);

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
    worker: JsValue,
    available: bool,
}

std::thread_local! {
    static WEB_WORKER_POOL: RefCell<Vec<WebWorker>>
        = RefCell::new(Vec::new());
}

#[wasm_bindgen()]
pub fn register_web_worker(web_worker: JsValue) -> u32 {
    WEB_WORKER_POOL.with(|u| {
        let mut workers = u.borrow_mut();
        workers.push(WebWorker {
            worker: web_worker,
            available: false,
        });
        workers.len() - 1
    }) as u32
}

#[wasm_bindgen()]
pub fn return_web_worker(id: u32) {
    WEB_WORKER_POOL.with(|u| {
        let mut workers = u.borrow_mut();
        let worker = workers.get_mut(id as usize);
        if let Some(worker) = worker {
            worker.available = true;
        }
    });
}

#[wasm_bindgen()]
pub fn get_web_worker(id: u32) -> JsValue {
    WEB_WORKER_POOL.with(|u| {
        let workers = u.borrow();
        if let Some(worker) = workers.get(id as usize) {
            worker.worker.clone()
        } else {
            JsValue::NULL
        }
    })
}

#[wasm_bindgen()]
pub fn claim_web_worker() -> JsValue {
    WEB_WORKER_POOL.with(|u| {
        let mut workers = u.borrow_mut();
        for (n, worker) in workers.iter_mut().enumerate() {
            if worker.available {
                worker.available = false;
                return JsValue::from(n);
            }
        }
        JsValue::NULL
    })
}

#[wasm_bindgen()]
pub async fn schedule_wasm_task(
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
            opts.name(&*format!("Wasm-Thread"));

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
            let result = wasm_bindgen_futures::JsFuture::from(start_wasm(
                wasm_bindgen::module()
                    .dyn_into::<js_sys::WebAssembly::Module>()
                    .unwrap(),
                wasm_bindgen::memory(),
                JsValue::from(task as u32),
                opts,
                wasm_module,
                wasm_memory,
                wasm_cache,
            ));
            _process_worker_result(result, None).await
        }
    }
}
