use std::time::Duration;

use js_sys::{Function, Promise};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::{Executor, Host, LocalFuture};

#[wasm_bindgen]
extern "C" {
    // Globals present in windows, workers and node alike.
    #[wasm_bindgen(js_namespace = Date, js_name = now)]
    fn date_now() -> f64;

    #[wasm_bindgen(js_namespace = performance, js_name = now)]
    fn performance_now() -> f64;

    #[wasm_bindgen(js_name = setTimeout)]
    fn set_timeout(handler: &Function, millis: f64) -> JsValue;

    #[wasm_bindgen(js_namespace = crypto, js_name = getRandomValues)]
    fn get_random_values(buf: &mut [u8]);
}

/// `setTimeout` treats longer delays as zero.
const MAX_TIMEOUT_MS: f64 = i32::MAX as f64;

/// `crypto.getRandomValues` rejects larger requests with a `QuotaExceededError`.
const MAX_RANDOM_CHUNK: usize = 65536;

/// The host on a JavaScript event loop (browser window, worker or node), for
/// `wasm32-unknown-unknown`.
///
/// - `CLOCK_REALTIME` is `Date.now()`, so it has millisecond resolution.
/// - `CLOCK_MONOTONIC` is `performance.now()`, which browsers coarsen (to 100 µs, or 5 µs when
///   cross-origin isolated).
/// - Sleeps use `setTimeout`, re-armed until the monotonic clock reaches the deadline, since timers
///   can fire early by the clock's rounding and delays above 2^31 - 1 ms do not work.
/// - Entropy is `crypto.getRandomValues`.
/// - Background futures run on `wasm_bindgen_futures::spawn_local`. Blocking work runs inline
///   when awaited: the JavaScript thread has nowhere else to put it, so it stalls the event loop
///   for its duration.
#[derive(Default)]
pub struct WebHost;

impl WebHost {
    pub fn new() -> Self {
        Self
    }
}

impl Host for WebHost {
    fn realtime(&self) -> Duration {
        Duration::from_secs_f64(date_now().max(0.0) / 1000.0)
    }

    fn monotonic(&self) -> Duration {
        Duration::from_secs_f64(performance_now().max(0.0) / 1000.0)
    }

    fn sleep(&self, duration: Duration) -> LocalFuture<'_, ()> {
        self.sleep_until(self.monotonic().saturating_add(duration))
    }

    fn sleep_until(&self, deadline: Duration) -> LocalFuture<'_, ()> {
        Box::pin(async move {
            loop {
                let now = self.monotonic();
                if now >= deadline {
                    return;
                }
                let millis = ((deadline - now).as_secs_f64() * 1000.0).ceil();
                timeout(millis.min(MAX_TIMEOUT_MS)).await;
            }
        })
    }

    fn random(&self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(MAX_RANDOM_CHUNK) {
            get_random_values(chunk);
        }
    }

    fn executor(&self) -> &dyn Executor {
        self
    }
}

impl Executor for WebHost {
    fn spawn_local(&self, future: LocalFuture<'static, ()>) {
        wasm_bindgen_futures::spawn_local(future);
    }

    fn spawn_blocking(&self, work: Box<dyn FnOnce() + Send>) -> LocalFuture<'static, ()> {
        Box::pin(async move { work() })
    }
}

/// Resolves after `millis` on the event loop. Dropping it leaves a timer that resolves nothing.
async fn timeout(millis: f64) {
    let promise = Promise::new(&mut |resolve, _reject| {
        set_timeout(&resolve, millis);
    });
    // The promise only ever resolves.
    let _ = JsFuture::from(promise).await;
}
