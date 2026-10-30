use std::{
    cell::{Cell, RefCell},
    io,
    rc::Rc,
    task::Poll,
    time::Duration,
};

use js_sys::{Function, Uint8Array};
use wasm_bindgen::prelude::*;

use crate::web_timer::{CLOCK, delay_ms, random_chunks};
use crate::{Executor, Host, LocalFuture, ThreadStart};

#[wasm_bindgen]
extern "C" {
    // Globals present in windows, workers and node alike.
    #[wasm_bindgen(js_namespace = Date, js_name = now)]
    fn date_now() -> f64;

    #[wasm_bindgen(js_namespace = performance, js_name = now)]
    fn performance_now() -> f64;

    #[wasm_bindgen(js_name = clearTimeout)]
    fn clear_timeout(id: &JsValue);

    #[wasm_bindgen(js_name = setTimeout)]
    fn set_timeout(handler: &Function, millis: f64) -> JsValue;

    #[wasm_bindgen(js_namespace = crypto, js_name = getRandomValues)]
    fn get_random_values(buf: &Uint8Array);
}

#[wasm_bindgen(
    inline_js = "export function performanceOrigin() { return performance.timeOrigin; }"
)]
extern "C" {
    #[wasm_bindgen(js_name = performanceOrigin)]
    fn performance_origin() -> f64;
}

pub(crate) fn monotonic() -> Duration {
    CLOCK
        .lock()
        .unwrap()
        .sample(performance_origin(), performance_now())
}

/// The host on a JavaScript event loop (browser window, worker or node), for
/// `wasm32-unknown-unknown`.
///
/// - `CLOCK_REALTIME` is `Date.now()`, so it has millisecond resolution.
/// - `CLOCK_MONOTONIC` normalizes `performance.timeOrigin + performance.now()` to a shared
///   epoch and clamps backwards samples, which browsers coarsen (to 100 µs, or 5 µs when
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
        monotonic()
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
                timeout(delay_ms(deadline - now)).await;
            }
        })
    }

    fn random(&self, buf: &mut [u8]) {
        random_chunks(buf, |chunk| {
            // Web Crypto rejects SharedArrayBuffer views. Keep JS scratch bounded by
            // its 65536-byte quota and copy into the shared runtime heap afterwards.
            let scratch = Uint8Array::new_with_length(chunk.len() as u32);
            get_random_values(&scratch);
            scratch.copy_to(chunk);
        });
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

    // Threaded embedders use BrowserHost and its coordinator-created workers.
    fn spawn_thread(&self, start: ThreadStart) -> io::Result<()> {
        drop(start);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "WebHost is single-event-loop; use BrowserHost for guest threads",
        ))
    }
}

/// Own the callback and handle together. A canceled JsFuture/Promise would retain
/// Rust callbacks until resolution; clearing that timer would then leak those roots.
async fn timeout(millis: f64) {
    struct Alarm {
        id: JsValue,
        _callback: Closure<dyn FnMut()>,
    }
    impl Drop for Alarm {
        fn drop(&mut self) {
            clear_timeout(&self.id);
        }
    }
    let done = Rc::new(Cell::new(false));
    let waker = Rc::new(RefCell::new(None::<std::task::Waker>));
    let completed = done.clone();
    let wake = waker.clone();
    let callback = Closure::new(move || {
        completed.set(true);
        let waker = wake.borrow_mut().take();
        if let Some(waker) = waker {
            waker.wake();
        }
    });
    let id = set_timeout(callback.as_ref().unchecked_ref(), millis);
    let _alarm = Alarm {
        id,
        _callback: callback,
    };
    std::future::poll_fn(|cx| {
        *waker.borrow_mut() = Some(cx.waker().clone());
        if done.get() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
}

/// A threaded browser host. Construct on a coordinator worker after async bootstrap.
/// Deadlines run on an independent event-loop worker, even while this worker parks in
/// Atomics.wait or executes guest code. No browser-native thread CPU clock is available;
/// exec uses its existing active-wall-time accounting fallback.
#[cfg(feature = "browser")]
#[derive(Clone)]
pub struct BrowserHost {
    executor: crate::browser::BrowserExecutor,
}

#[cfg(feature = "browser")]
impl BrowserHost {
    pub fn new(executor: crate::browser::BrowserExecutor) -> io::Result<Self> {
        #[cfg(target_arch = "wasm32")]
        ensure_timers()
            .map_err(|error| io::Error::other(format!("browser timer driver: {error:?}")))?;
        Ok(Self { executor })
    }

    /// Kernel timer and TTY ThreadStarts must use reserved service capacity.
    pub fn services(&self) -> Self {
        Self {
            executor: self.executor.services(),
        }
    }
}

#[cfg(feature = "browser")]
static TIMERS: std::sync::LazyLock<crate::web_timer::Timers> =
    std::sync::LazyLock::new(|| crate::web_timer::Timers::new(rearm_timers));

#[cfg(feature = "browser")]
impl Host for BrowserHost {
    fn realtime(&self) -> Duration {
        WebHost.realtime()
    }
    fn monotonic(&self) -> Duration {
        monotonic()
    }
    fn random(&self, buf: &mut [u8]) {
        WebHost.random(buf);
    }
    fn executor(&self) -> &dyn Executor {
        &self.executor
    }
    fn sleep(&self, duration: Duration) -> LocalFuture<'_, ()> {
        self.sleep_until(self.monotonic().saturating_add(duration))
    }
    fn sleep_until(&self, deadline: Duration) -> LocalFuture<'_, ()> {
        self.sleep_until_range(deadline, Duration::ZERO)
    }
    fn sleep_until_range(&self, deadline: Duration, slack: Duration) -> LocalFuture<'_, ()> {
        Box::pin(TIMERS.sleep(deadline, slack))
    }
}

#[cfg(feature = "browser")]
#[wasm_bindgen(module = "/src/browser/timer-bridge.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = ensureTimers)]
    fn ensure_timers() -> Result<(), JsValue>;
    #[wasm_bindgen(js_name = rearmTimers)]
    fn rearm_timers();
}

/// Trusted event worker entry; Wakers contain only B2's shared atomic scheduler state.
/// Returns -1 for an empty queue; early/coarse alarms rearm instead of completing early.
#[cfg(all(feature = "browser", target_arch = "wasm32"))]
#[wasm_bindgen]
pub fn browser_timers_tick() -> f64 {
    let now = monotonic();
    TIMERS
        .tick(now)
        .map_or(-1.0, |hard| delay_ms(hard.saturating_sub(monotonic())))
}

/// Shutdown assertion and embedding diagnostics; does not expose queue internals.
#[cfg(all(feature = "browser", target_arch = "wasm32"))]
#[wasm_bindgen]
pub fn browser_timers_pending() -> usize {
    TIMERS.pending()
}
