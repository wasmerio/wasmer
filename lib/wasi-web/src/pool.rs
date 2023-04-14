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
use wasmer_wasix::{
    runtime::SpawnType,
    wasmer::{AsJs, Memory, MemoryType, Module, Store, WASM_MAX_PAGES},
    VirtualTaskManager, WasiThreadError,
};
use web_sys::{DedicatedWorkerGlobalScope, WorkerOptions, WorkerType};
use xterm_js_rs::Terminal;

use super::{common::*, interval::*};
use crate::runtime::WebTaskManager;

pub type BoxRun<'a> = Box<dyn FnOnce() + Send + 'a>;

pub type BoxRunAsync<'a, T> =
    Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = T> + 'static>> + Send + 'a>;

#[derive(Debug, Clone, Copy)]
enum WasmRunType {
    Create,
    CreateWithMemory(MemoryType),
    Existing(MemoryType),
}

#[derive(Derivative)]
#[derivative(Debug)]
struct WasmRunCommand {
    #[derivative(Debug = "ignore")]
    run: Box<dyn FnOnce(Store, Module, Option<Memory>) + Send + 'static>,
    ty: WasmRunType,
    store: Store,
    module_bytes: Bytes,
}

enum WasmRunMemory {
    WithoutMemory,
    WithMemory(MemoryType),
}

struct WasmRunContext {
    cmd: WasmRunCommand,
    memory: WasmRunMemory,
}

trait AssertSendSync: Send + Sync {}
impl AssertSendSync for WebThreadPool {}

#[wasm_bindgen]
#[derive(Debug)]
pub struct WebThreadPoolInner {
    pool_reactors: Arc<PoolState>,
    pool_dedicated: Arc<PoolState>,
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

enum Message {
    Run(BoxRun<'static>),
    RunAsync(BoxRunAsync<'static, ()>),
}

impl Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Message::Run(_) => write!(f, "run"),
            Message::RunAsync(_) => write!(f, "run-async"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PoolType {
    Shared,
    Dedicated,
}

#[derive(Derivative)]
#[derivative(Debug)]
struct IdleThread {
    idx: usize,
    #[derivative(Debug = "ignore")]
    work: mpsc::UnboundedSender<Message>,
}

impl IdleThread {
    #[allow(dead_code)]
    fn consume(self, msg: Message) {
        let _ = self.work.send(msg);
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct PoolState {
    #[derivative(Debug = "ignore")]
    idle_rx: Mutex<mpsc::UnboundedReceiver<IdleThread>>,
    idle_tx: mpsc::UnboundedSender<IdleThread>,
    idx_seed: AtomicUsize,
    idle_size: usize,
    blocking: bool,
    spawn: mpsc::UnboundedSender<Message>,
    #[allow(dead_code)]
    type_: PoolType,
}

pub struct ThreadState {
    pool: Arc<PoolState>,
    #[allow(dead_code)]
    idx: usize,
    tx: mpsc::UnboundedSender<Message>,
    rx: Mutex<Option<mpsc::UnboundedReceiver<Message>>>,
    init: Mutex<Option<Message>>,
}

#[wasm_bindgen]
pub struct LoaderHelper {}
#[wasm_bindgen]
impl LoaderHelper {
    #[wasm_bindgen(js_name = mainJS)]
    pub fn main_js(&self) -> JsString {
        #[wasm_bindgen]
        extern "C" {
            #[wasm_bindgen(js_namespace = ["import", "meta"], js_name = url)]
            static URL: JsString;
        }

        URL.clone()
    }
}

#[wasm_bindgen(module = "/public/worker.js")]
extern "C" {
    #[wasm_bindgen(js_name = "startWorker")]
    fn start_worker(
        module: JsValue,
        memory: JsValue,
        shared_data: JsValue,
        opts: WorkerOptions,
        builder: LoaderHelper,
    ) -> Promise;

    #[wasm_bindgen(js_name = "startWasm")]
    fn start_wasm(
        module: JsValue,
        memory: JsValue,
        ctx: JsValue,
        opts: WorkerOptions,
        builder: LoaderHelper,
        wasm_module: JsValue,
        wasm_memory: JsValue,
    ) -> Promise;

    #[wasm_bindgen(js_name = "scheduleTask")]
    fn schedule_task(task: JsValue, module: JsValue, memory: JsValue);
}

impl WebThreadPool {
    pub fn new(size: usize) -> Result<WebThreadPool, JsValue> {
        info!("pool::create(size={})", size);

        let (idle_tx1, idle_rx1) = mpsc::unbounded_channel();
        let (idle_tx3, idle_rx3) = mpsc::unbounded_channel();

        let (spawn_tx1, mut spawn_rx1) = mpsc::unbounded_channel();
        let (spawn_tx3, mut spawn_rx3) = mpsc::unbounded_channel();

        let pool_reactors = PoolState {
            idle_rx: Mutex::new(idle_rx1),
            idle_tx: idle_tx1,
            idx_seed: AtomicUsize::new(0),
            blocking: false,
            idle_size: 2usize.max(size),
            type_: PoolType::Shared,
            spawn: spawn_tx1,
        };

        let pool_dedicated = PoolState {
            idle_rx: Mutex::new(idle_rx3),
            idle_tx: idle_tx3,
            idx_seed: AtomicUsize::new(0),
            blocking: true,
            idle_size: 1usize.max(size),
            type_: PoolType::Dedicated,
            spawn: spawn_tx3,
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
                    spawn = spawn_rx1.recv() => {
                        if let Some(spawn) = spawn { inner1.pool_reactors.expand(spawn); } else { break; }
                    }
                    spawn = spawn_rx3.recv() => {
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
        self.inner.pool_reactors.spawn(Message::RunAsync(task));
    }

    pub fn spawn_wasm(
        &self,
        run: impl FnOnce(Store, Module, Option<Memory>) + Send + 'static,
        wasm_store: Store,
        wasm_module: Module,
        spawn_type: SpawnType,
    ) -> Result<(), WasiThreadError> {
        let mut wasm_memory = JsValue::null();
        let run_type = match spawn_type {
            SpawnType::Create => WasmRunType::Create,
            SpawnType::CreateWithType(mem) => WasmRunType::CreateWithMemory(mem.ty),
            SpawnType::NewThread(memory) => {
                wasm_memory = memory.as_jsvalue(&wasm_store);
                WasmRunType::Existing(memory.ty(&wasm_store))
            }
        };

        let task = Box::new(WasmRunCommand {
            run: Box::new(move |store, module, memory| {
                run(store, module, memory);
            }),
            ty: run_type,
            store: wasm_store,
            module_bytes: wasm_module.serialize().unwrap(),
        });
        let task = Box::into_raw(task);

        schedule_task(
            JsValue::from(task as u32),
            JsValue::from(wasm_module),
            wasm_memory,
        );
        Ok(())
    }

    pub fn spawn_dedicated(&self, task: BoxRun<'static>) {
        self.inner.pool_dedicated.spawn(Message::Run(task));
    }

    pub fn spawn_dedicated_async(&self, task: BoxRunAsync<'static, ()>) {
        self.inner.pool_dedicated.spawn(Message::RunAsync(task));
    }
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

impl PoolState {
    fn spawn(&self, msg: Message) {
        for _ in 0..10 {
            if let Ok(mut guard) = self.idle_rx.try_lock() {
                if let Ok(thread) = guard.try_recv() {
                    thread.consume(msg);
                    return;
                }
                break;
            }
            std::thread::yield_now();
        }

        self.spawn.send(msg).unwrap();
    }

    fn expand(self: &Arc<Self>, init: Message) {
        let (tx, rx) = mpsc::unbounded_channel();
        let idx = self.idx_seed.fetch_add(1usize, Ordering::Release);
        let state = Arc::new(ThreadState {
            pool: Arc::clone(self),
            idx,
            tx,
            rx: Mutex::new(Some(rx)),
            init: Mutex::new(Some(init)),
        });
        Self::start_worker_now(idx, state, None);
    }

    pub fn start_worker_now(
        idx: usize,
        state: Arc<ThreadState>,
        should_warn_on_error: Option<Terminal>,
    ) {
        let mut opts = WorkerOptions::new();
        opts.type_(WorkerType::Module);
        opts.name(&*format!("Worker-{:?}-{}", state.pool.type_, idx));

        let ptr = Arc::into_raw(state);

        let result = wasm_bindgen_futures::JsFuture::from(start_worker(
            wasm_bindgen::module(),
            wasm_bindgen::memory(),
            JsValue::from(ptr as u32),
            opts,
            LoaderHelper {},
        ));

        wasm_bindgen_futures::spawn_local(async move {
            _process_worker_result(result, should_warn_on_error).await;
        });
    }
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

impl ThreadState {
    fn work(state: Arc<ThreadState>) {
        let thread_index = state.idx;
        info!(
            "worker started (index={}, type={:?})",
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
                    match task {
                        Message::Run(task) => {
                            task();
                        }
                        Message::RunAsync(task) => {
                            let future = task();
                            if pool.blocking {
                                future.await;
                            } else {
                                wasm_bindgen_futures::spawn_local(async move {
                                    future.await;
                                });
                            }
                        }
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
                let idle = IdleThread {
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
    ThreadState::work(state);
}

#[wasm_bindgen(skip_typescript)]
pub fn wasm_entry_point(ctx_ptr: u32, wasm_module: JsValue, wasm_memory: JsValue) {
    // Grab the run wrapper that passes us the rust variables (and extract the callback)
    let ctx = ctx_ptr as *mut WasmRunContext;
    let ctx = unsafe { Box::from_raw(ctx) };
    let run_callback = (*ctx).cmd.run;

    // Compile the web assembly module
    let mut wasm_store = ctx.cmd.store;
    let wasm_module = match wasm_module.dyn_into::<js_sys::WebAssembly::Module>() {
        Ok(a) => a,
        Err(err) => {
            error!(
                "Failed to receive module - {}",
                err.as_string().unwrap_or_else(|| format!("{:?}", err))
            );
            return;
        }
    };
    let wasm_module: Module = (wasm_module, ctx.cmd.module_bytes.clone()).into();

    // If memory was passed to the web worker then construct it
    let wasm_memory = match ctx.memory {
        WasmRunMemory::WithoutMemory => None,
        WasmRunMemory::WithMemory(wasm_memory_type) => {
            let wasm_memory =
                match Memory::from_jsvalue(&mut wasm_store, &wasm_memory_type, &wasm_memory) {
                    Ok(a) => a,
                    Err(err) => {
                        // error!(
                        //     "Failed to receive memory for module - {}",
                        //     err.as_string().unwrap_or_else(|| format!("{:?}", err))
                        // );
                        return;
                    }
                };
            Some(wasm_memory)
        }
    };

    let name = js_sys::global()
        .unchecked_into::<DedicatedWorkerGlobalScope>()
        .name();
    debug!("{}: Entry", name);

    // Invoke the callback which will run the web assembly module
    run_callback(wasm_store, wasm_module, wasm_memory);
}

#[wasm_bindgen()]
pub fn worker_schedule_task(task_ptr: u32, wasm_module: JsValue, mut wasm_memory: JsValue) {
    // Grab the task that passes us the rust variables
    let task = task_ptr as *mut WasmRunCommand;
    let mut task = unsafe { Box::from_raw(task) };

    let mut opts = WorkerOptions::new();
    opts.type_(WorkerType::Module);
    opts.name(&*format!("WasmWorker"));

    let result = match task.ty.clone() {
        WasmRunType::Create => {
            let ctx = WasmRunContext {
                cmd: *task,
                memory: WasmRunMemory::WithoutMemory,
            };
            let ctx = Box::into_raw(Box::new(ctx));

            wasm_bindgen_futures::JsFuture::from(start_wasm(
                wasm_bindgen::module(),
                wasm_bindgen::memory(),
                JsValue::from(ctx as u32),
                opts,
                LoaderHelper {},
                wasm_module,
                wasm_memory,
            ))
        }
        WasmRunType::CreateWithMemory(ty) => {
            if ty.shared == false {
                // We can only pass memory around between web workers when its a shared memory
                error!("Failed to create WASM process with external memory as only shared memory is supported yet this web assembly binary imports non-shared memory.");
                return;
            }
            if ty.maximum.is_none() {
                // Browsers require maximum number defined on shared memory
                error!("Failed to create WASM process with external memory as shared memory must have a maximum size however this web assembly binary imports shared memory with no maximum defined.");
                return;
            }

            if wasm_memory.is_null() {
                let memory = match Memory::new(&mut task.store, ty.clone()) {
                    Ok(a) => a,
                    Err(err) => {
                        error!("Failed to create WASM memory - {}", err);
                        return;
                    }
                };
                wasm_memory = memory.as_jsvalue(&task.store);
            }

            let ctx = WasmRunContext {
                cmd: *task,
                memory: WasmRunMemory::WithMemory(ty),
            };
            let ctx = Box::into_raw(Box::new(ctx));

            wasm_bindgen_futures::JsFuture::from(start_wasm(
                wasm_bindgen::module(),
                wasm_bindgen::memory(),
                JsValue::from(ctx as u32),
                opts,
                LoaderHelper {},
                wasm_module,
                wasm_memory,
            ))
        }
        WasmRunType::Existing(wasm_memory_type) => {
            let ctx = WasmRunContext {
                cmd: *task,
                memory: WasmRunMemory::WithMemory(wasm_memory_type),
            };
            let ctx = Box::into_raw(Box::new(ctx));

            wasm_bindgen_futures::JsFuture::from(start_wasm(
                wasm_bindgen::module(),
                wasm_bindgen::memory(),
                JsValue::from(ctx as u32),
                opts,
                LoaderHelper {},
                wasm_module,
                wasm_memory,
            ))
        }
    };

    wasm_bindgen_futures::spawn_local(async move { _process_worker_result(result, None).await });
}
