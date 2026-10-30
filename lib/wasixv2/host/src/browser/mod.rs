//! Dedicated workers over one shared Rust heap. Build with the recipe in
//! `tests/browser/README.md`; this is separate from the single-event-loop WebHost.
//!
//! The JS coordinator is created asynchronously before entering Rust. Only its workers
//! may construct an executor. Engine objects use worker-local envelope callbacks; no
//! wasm-bindgen slab index or JS reference crosses workers through the Rust heap.

// The transport is wasm-only; compile its ownership model natively for regression tests.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod admission;
mod scheduler;

use std::{
    io,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Poll, Waker},
};

use crate::{Executor, LocalFuture, ThreadStart};
use admission::{Admissions, Capacity};
pub use scheduler::{Parker, block_on};

static ADMISSIONS: LazyLock<Admissions> = LazyLock::new(Admissions::default);

struct Pool {
    tasks: Arc<Capacity>,
    services: Arc<Capacity>,
    closed: AtomicBool,
}

/// Threaded browser executor. Construction requires a bootstrapped coordinator worker
/// connection (see `coordinator.js`). Limits bound live admissions, including execution;
/// a new child never queues behind an occupied guest worker.
#[derive(Clone)]
pub struct BrowserExecutor {
    pool: Arc<Pool>,
    service: bool,
}

impl BrowserExecutor {
    /// The async JS bootstrap must finish before calling this on a runtime worker.
    pub fn new(task_limit: usize, service_limit: usize) -> io::Result<Self> {
        #[cfg(target_arch = "wasm32")]
        ensure_worker().map_err(js_error)?;
        Ok(Self {
            pool: Arc::new(Pool {
                tasks: Capacity::new(task_limit),
                services: Capacity::new(service_limit),
                closed: AtomicBool::new(false),
            }),
            service: false,
        })
    }

    /// Reserved driver capacity. A kernel Host must expose this executor while exec uses
    /// the task executor. Driver-start failure is fatal, so timer/TTY code cannot fall
    /// back onto the blocking worker. No change to the Host/Executor/ThreadStart contract.
    pub fn services(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            service: true,
        }
    }

    /// Refuse new admissions. The embedding layer must first stop its Machines, wake
    /// blocked tasks, drain output, and await worker completion before JS shutdown.
    pub fn close(&self) {
        self.pool.closed.store(true, Ordering::Release);
    }

    fn admit(&self, start: ThreadStart) -> io::Result<()> {
        if self.pool.closed.load(Ordering::Acquire) {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let capacity = if self.service {
            &self.pool.services
        } else {
            &self.pool.tasks
        };
        let ticket = ADMISSIONS.reserve(capacity, start)?;
        #[cfg(target_arch = "wasm32")]
        {
            let envelope = OBJECT_HOOKS
                .with_borrow(|hooks| (hooks.prepare)())
                .map_err(js_error)?;
            prepare_worker(ticket.id, self.service, envelope).map_err(js_error)?;
            ticket.accept();
            // A successful post is the ownership handoff. Before that, Ticket drops
            // the unclaimed closure exactly once on any bootstrap/import/clone error.
            commit_worker(ticket.id).map_err(js_error)?;
            ticket.commit();
            Ok(())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            drop(ticket);
            Err(io::ErrorKind::Unsupported.into())
        }
    }
}

/// Spawn a non-Send future on the current worker with bounded admission. The trait's
/// infallible spawn_local makes exhaustion fatal; embedders can use this fallible form.
pub fn try_spawn_local(future: LocalFuture<'static, ()>) -> io::Result<()> {
    scheduler::spawn(future)
}

impl Executor for BrowserExecutor {
    fn spawn_local(&self, future: LocalFuture<'static, ()>) {
        try_spawn_local(future).expect("browser local task capacity exhausted");
    }

    fn spawn_thread(&self, start: ThreadStart) -> io::Result<()> {
        let result = self.admit(start);
        if self.service {
            // Linux kernel/kthread.c:kthread_create_on_node reports creation failure;
            // an indispensable browser driver cannot be scheduled on its blocked caller.
            assert!(
                result.is_ok(),
                "browser kernel driver admission failed: {result:?}"
            );
        }
        result
    }

    fn spawn_blocking(&self, work: Box<dyn FnOnce() + Send>) -> LocalFuture<'static, ()> {
        #[derive(Default)]
        struct Completion {
            done: AtomicBool,
            waker: Mutex<Option<Waker>>,
        }
        let completion = Arc::new(Completion::default());
        let out = completion.clone();
        let result = self.spawn_thread(Box::new(move || {
            Box::pin(async move {
                work();
                out.done.store(true, Ordering::Release);
                let waker = out.waker.lock().unwrap().take();
                if let Some(waker) = waker {
                    waker.wake();
                }
            })
        }));
        Box::pin(async move {
            result.expect("browser blocking worker admission failed");
            std::future::poll_fn(|cx| {
                *completion.waker.lock().unwrap() = Some(cx.waker().clone());
                if completion.done.load(Ordering::Acquire) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await;
        })
    }
}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// The embedding crate installs these callbacks on each worker before admitting work.
/// They should wrap the engine's prepare/receive_shared_object_message and collection.
/// Callbacks execute locally; only their structured-clone envelope is transported.
#[cfg(target_arch = "wasm32")]
pub struct ObjectHooks {
    pub prepare: fn() -> Result<JsValue, JsValue>,
    pub receive: fn(JsValue) -> Result<(), JsValue>,
    pub collect: fn(),
    /// Release every remaining worker-local engine/JS root before TLS destruction.
    pub teardown: fn(),
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static OBJECT_HOOKS: std::cell::RefCell<ObjectHooks> = const { std::cell::RefCell::new(ObjectHooks {
        prepare: || Ok(JsValue::UNDEFINED),
        receive: |_| Ok(()),
        collect: || {},
        teardown: || {},
    }) };
}

/// Set engine envelope hooks on the current worker, during its bootstrap.
#[cfg(target_arch = "wasm32")]
pub fn set_object_hooks(hooks: ObjectHooks) {
    OBJECT_HOOKS.with_borrow_mut(|current| *current = hooks);
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(module = "/src/browser/bridge.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = ensureWorker)]
    fn ensure_worker() -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = prepareWorker)]
    fn prepare_worker(id: u32, service: bool, envelope: JsValue) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = commitWorker)]
    fn commit_worker(id: u32) -> Result<(), JsValue>;
    #[wasm_bindgen(js_name = waitWord)]
    fn js_wait_word(memory: JsValue, address: usize, observed: i32);
    #[wasm_bindgen(js_name = notifyWord)]
    fn js_notify_word(memory: JsValue, address: usize);
}

#[cfg(target_arch = "wasm32")]
fn js_error(error: JsValue) -> io::Error {
    io::Error::other(format!("browser worker admission: {error:?}"))
}

#[cfg(target_arch = "wasm32")]
fn wait_word(word: &std::sync::atomic::AtomicI32, observed: i32) {
    js_wait_word(wasm_bindgen::memory(), word as *const _ as usize, observed);
}

#[cfg(target_arch = "wasm32")]
fn notify_word(word: &std::sync::atomic::AtomicI32) {
    js_notify_word(wasm_bindgen::memory(), word as *const _ as usize);
}

/// Trusted coordinator entry: import engine objects before acknowledging bootstrap.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn browser_receive_envelope(envelope: JsValue) -> Result<(), JsValue> {
    OBJECT_HOOKS.with_borrow(|hooks| (hooks.receive)(envelope))
}

/// Trusted coordinator entry: IDs are scoped to this shared runtime, never reused.
/// A duplicate, stale, canceled or unaccepted dispatch is rejected before calling user code.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn browser_run(id: u32) -> Result<(), JsValue> {
    let (start, lease) = ADMISSIONS
        .claim(id)
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    block_on(async move { start().await });
    drop(lease);
    OBJECT_HOOKS.with_borrow(|hooks| (hooks.collect)());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_refuses_new_threads_and_services_have_reserved_capacity() {
        let executor = BrowserExecutor::new(0, 1).unwrap();
        assert!(Arc::ptr_eq(&executor.pool, &executor.services().pool));
        assert_eq!(
            executor
                .spawn_thread(Box::new(|| Box::pin(async {})))
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        executor.close();
        assert_eq!(
            executor
                .spawn_thread(Box::new(|| Box::pin(async {})))
                .unwrap_err()
                .kind(),
            io::ErrorKind::BrokenPipe
        );
    }

    #[test]
    #[should_panic(expected = "browser kernel driver admission failed")]
    fn service_admission_never_falls_back_to_the_calling_worker() {
        BrowserExecutor::new(0, 0)
            .unwrap()
            .services()
            .spawn_thread(Box::new(|| Box::pin(async {})))
            .unwrap();
    }
}

// The pinned build-std recipe exposes std's existing destructor runner. Merely
// freeing bindgen TLS storage would leak std::thread_local! values on wasm.
#[cfg(target_arch = "wasm32")]
unsafe extern "C" {
    fn __wasixv2_thread_cleanup();
}

/// Final Rust call on this worker, after all task/local futures have completed.
/// JS must then destroy the allocated bindgen thread and terminate the worker.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn browser_retire() {
    OBJECT_HOOKS.with_borrow(|hooks| (hooks.teardown)());
    // SAFETY: trusted coordinator sends retire only to an idle worker, once. No
    // Rust work or references into TLS survive this call (including envelope roots).
    unsafe { __wasixv2_thread_cleanup() };
}
