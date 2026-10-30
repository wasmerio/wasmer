//! Synchronous async-host calls on dedicated workers. Promise-producing I/O must run
//! on another worker; parking this worker cannot drive its JavaScript event loop.

use std::{
    cell::Cell,
    future::Future,
    pin::{Pin, pin},
    sync::{
        Arc,
        atomic::{AtomicI32, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
};

use crate::RuntimeError;
use wasm_bindgen::JsCast;

/// Drives a borrowed future on its owning task worker until completion. The driver must
/// support nesting, cross-worker wakers, and wake-before-wait. It must never await a JS
/// Promise on the blocked worker. B2's scheduler can also poll worker-local Rust tasks.
pub type BlockingExecutor = fn(Pin<&mut dyn Future<Output = ()>>) -> Result<(), RuntimeError>;

thread_local! {
    static EXECUTOR: Cell<BlockingExecutor> = const { Cell::new(park_on_atomics) };
}

/// Installs the runtime's synchronous scheduler driver on the current worker, returning
/// the previous driver. Install on every task worker before entering guest code.
pub fn set_blocking_executor(executor: BlockingExecutor) -> BlockingExecutor {
    EXECUTOR.replace(executor)
}

pub(crate) fn block_on<T>(future: impl Future<Output = T>) -> Result<T, RuntimeError> {
    let mut result = None;
    {
        let mut task = pin!(async { result = Some(future.await) });
        EXECUTOR.get()(task.as_mut())?;
    }
    result
        .ok_or_else(|| RuntimeError::new("blocking executor returned before completing its future"))
}

#[derive(Default)]
struct Parker(AtomicI32);

impl Parker {
    fn word(&self) -> Result<js_sys::Int32Array, RuntimeError> {
        let memory = wasm_bindgen::memory().unchecked_into::<js_sys::WebAssembly::Memory>();
        let buffer = memory.buffer();
        if !buffer.is_instance_of::<js_sys::SharedArrayBuffer>() {
            return Err(RuntimeError::new(
                "a pending synchronous host call requires a shared runtime heap and a task worker",
            ));
        }
        Ok(js_sys::Int32Array::new_with_byte_offset_and_length(
            &buffer,
            &self.0 as *const AtomicI32 as u32,
            1,
        ))
    }
}

impl Wake for Parker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::Release);
        if let Ok(word) = self.word() {
            // The atomic predicate retains the wake even when nobody is waiting yet.
            let _ = js_sys::Atomics::notify(&word, 0);
        }
    }
}

fn park_on_atomics(future: Pin<&mut dyn Future<Output = ()>>) -> Result<(), RuntimeError> {
    drive(future, Arc::new(Parker::default()))
}

fn drive(
    future: Pin<&mut dyn Future<Output = ()>>,
    parker: Arc<Parker>,
) -> Result<(), RuntimeError> {
    let waker = Waker::from(parker.clone());
    let mut cx = Context::from_waker(&waker);
    let mut future = future;
    loop {
        // Linux kernel/sched/wait.c:prepare_to_wait_event and kernel/sched/core.c:
        // try_to_wake_up establish/recheck readiness before parking. These are runtime
        // scheduler words, never guest futex keys (kernel/futex/core.c:get_futex_key).
        let observed = parker.0.load(Ordering::Acquire);
        if let Poll::Ready(()) = future.as_mut().poll(&mut cx) {
            return Ok(());
        }
        if parker.0.load(Ordering::Acquire) != observed {
            continue;
        }
        js_sys::Atomics::wait(&parker.word()?, 0, observed).map_err(RuntimeError::from)?;
    }
}

#[cfg(all(test, target_feature = "atomics"))]
mod tests {
    use super::*;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen(
        inline_js = "export async function wakeOnWorker(memory, ready, sequence) { const url = URL.createObjectURL(new Blob([`onmessage = ({data: [memory, ready, sequence]}) => { setTimeout(() => { const words = new Int32Array(memory.buffer); Atomics.store(words, ready / 4, 1); Atomics.add(words, sequence / 4, 1); Atomics.notify(words, sequence / 4); postMessage('done'); }, 20); }; postMessage('ready');`], {type:'text/javascript'})); const worker = new Worker(url); URL.revokeObjectURL(url); let booted, completed; const boot = new Promise((resolve, reject) => { booted = resolve; worker.onerror = reject; }); const done = new Promise(resolve => completed = resolve); worker.onmessage = ({data}) => { if (data === 'ready') booted(); else completed(); }; await boot; worker.postMessage([memory, ready, sequence]); return {worker, done}; } export async function stopWakeWorker(handle) { await handle.done; handle.worker.terminate(); }"
    )]
    extern "C" {
        fn wakeOnWorker(memory: JsValue, ready: u32, sequence: u32) -> js_sys::Promise;
        fn stopWakeWorker(worker: JsValue) -> js_sys::Promise;
    }

    #[wasm_bindgen_test]
    async fn synchronous_driver_parks_and_observes_a_foreign_worker_wake() {
        let parker = Arc::new(Parker::default());
        let ready = AtomicI32::new(0);
        let worker = wasm_bindgen_futures::JsFuture::from(wakeOnWorker(
            wasm_bindgen::memory(),
            &ready as *const _ as u32,
            &parker.0 as *const _ as u32,
        ))
        .await
        .unwrap();
        let mut future = pin!(std::future::poll_fn(|_| {
            if ready.load(Ordering::Acquire) == 0 {
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        }));
        let result = drive(future.as_mut(), parker.clone());
        wasm_bindgen_futures::JsFuture::from(stopWakeWorker(worker))
            .await
            .unwrap();
        result.unwrap();
    }
}
