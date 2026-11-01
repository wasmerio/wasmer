//! Browser lifecycle regressions beside the facade's existing lifecycle_smoke.
use futures_util::{FutureExt, future::Either};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    time::Duration,
};
use wasixv2::{Engine, Net, NetConfig, ProcessConfig, Stdio, browser};
use wasixv2_host::{BrowserHost, Executor, Host, browser::BrowserExecutor};
use wasm_bindgen::prelude::*;

fn error(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

/// The primary Promise retains its callbacks and exact provider operation even
/// when a JsFuture waiter or the detached callback Promise is dropped.
#[wasm_bindgen]
pub fn jspi_driver_smoke() -> js_sys::Promise {
    use wasixv2_host::browser::future_to_promise;
    use wasm_bindgen::JsCast;
    let done = Arc::new(AtomicBool::new(false));
    let completed = done.clone();
    let observed = done.clone();
    let primary = future_to_promise(async move {
        let executor = BrowserExecutor::with_jspi(1, 1).map_err(error)?;
        let host = Arc::new(BrowserHost::new(executor.clone()).map_err(error)?);
        let ready = Arc::new(AtomicBool::new(false));
        let operation_ready = ready.clone();
        let wake = Arc::new(std::sync::Mutex::new(None::<Waker>));
        let operation_wake = wake.clone();
        let drops = Arc::new(AtomicUsize::new(0));
        let operation_drops = drops.clone();
        let starts = Arc::new(AtomicUsize::new(0));
        let operation_starts = starts.clone();
        struct Dropped(Arc<AtomicUsize>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
        let callback = future_to_promise(async move {
            let _dropped = Dropped(operation_drops);
            operation_starts.fetch_add(1, Ordering::Relaxed);
            std::future::poll_fn(|cx| {
                *operation_wake.lock().unwrap() = Some(cx.waker().clone());
                if operation_ready.load(Ordering::Acquire) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await;
            Ok(JsValue::UNDEFINED)
        });
        host.sleep(Duration::from_millis(5)).await;
        struct WakeProbe;
        impl Wake for WakeProbe {
            fn wake(self: Arc<Self>) {}
        }
        let probe = Arc::new(WakeProbe);
        let weak = Arc::downgrade(&probe);
        let waker = Waker::from(probe);
        let mut canceled = Box::pin(wasm_bindgen_futures::JsFuture::from(callback.clone()));
        assert!(
            canceled
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        drop(canceled);
        drop(waker);
        assert_eq!(starts.load(Ordering::Relaxed), 1);
        assert_eq!(
            drops.load(Ordering::Relaxed),
            0,
            "dropping a Promise waiter canceled its provider"
        );
        ready.store(true, Ordering::Release);
        wake.lock().unwrap().take().unwrap().wake();
        wasm_bindgen_futures::JsFuture::from(callback).await?;
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        assert!(
            weak.upgrade().is_none(),
            "settled callback retained the canceled waiter's waker"
        );
        let callback_host = host.clone();
        let detached = future_to_promise(async move {
            callback_host.sleep(Duration::from_millis(5)).await;
            completed.store(true, Ordering::Release);
            Ok(JsValue::UNDEFINED)
        });
        drop(detached);
        // The root body completes before the detached Promise's first poll.
        assert!(!observed.load(Ordering::Acquire));
        executor.close();
        Ok(JsValue::from_str(
            r#"{"registeredBeforePoll":true,"callbackDrained":true,"callbackWakerReleased":true,"providerDroppedOnce":true}"#,
        ))
    });
    let checked = Closure::once_into_js(move |result: JsValue| {
        assert!(
            done.load(Ordering::Acquire),
            "primary Promise settled before callback cleanup"
        );
        result
    });
    let then = js_sys::Reflect::get(&primary, &JsValue::from_str("then"))
        .unwrap()
        .unchecked_into::<js_sys::Function>();
    then.call1(&primary, &checked).unwrap().unchecked_into()
}

/// Measure host scheduling without weakening hrtimer_nanosleep's no-early rule
/// (Linux kernel/time/hrtimer.c). The runner verifies actual tab visibility.
#[wasm_bindgen]
pub fn background_sleep_smoke(count: u32, delay_ms: u32) -> Result<String, JsValue> {
    if !(1..=100).contains(&count) || !(1..=1000).contains(&delay_ms) {
        return Err(error("invalid background sleep sample bounds"));
    }
    wasixv2_host::browser::block_on(async {
        let executor = BrowserExecutor::new(8, 8).map_err(error)?;
        let host = BrowserHost::new(executor.clone()).map_err(error)?;
        let requested = Duration::from_millis(u64::from(delay_ms));
        let mut elapsed_ns = Vec::new();
        for _ in 0..count {
            let start = host.monotonic();
            host.sleep_until(start + requested).await;
            let elapsed = host.monotonic().saturating_sub(start);
            if elapsed < requested {
                return Err(error("background sleep completed before its deadline"));
            }
            elapsed_ns.push(elapsed.as_nanos() as u64);
        }
        executor.close();
        serde_json::to_string(&elapsed_ns).map_err(error)
    })
}

/// A coordinator task limit of one must reject the second admission, drop it
/// exactly once, then reuse the first worker after its accepted task completes.
#[wasm_bindgen]
pub fn admission_smoke() -> Result<String, JsValue> {
    wasixv2_host::browser::block_on(async {
        let executor = BrowserExecutor::new(2, 2).map_err(error)?;
        let host = Arc::new(BrowserHost::new(executor.clone()).map_err(error)?);
        let gate = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        let child_gate = gate.clone();
        let child_done = done.clone();
        let child_host = host.clone();
        executor
            .spawn_thread(Box::new(move || {
                Box::pin(async move {
                    while !child_gate.load(Ordering::Acquire) {
                        child_host.sleep(Duration::from_millis(1)).await;
                    }
                    child_done.store(true, Ordering::Release);
                })
            }))
            .map_err(error)?;
        struct Dropped(Arc<AtomicUsize>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let dropped = Dropped(drops.clone());
        let rejected = executor.spawn_thread(Box::new(move || {
            drop(dropped);
            Box::pin(async { panic!("rejected task executed") })
        }));
        assert!(rejected.is_err(), "runner must configure one task worker");
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        gate.store(true, Ordering::Release);
        while !done.load(Ordering::Acquire) {
            host.sleep(Duration::from_millis(1)).await;
        }
        executor.close();
        Ok(r#"{"admissionRejected":true,"droppedOnce":true}"#.into())
    })
}

/// Repeated canceled waits/kill/deadlines and timers must release their queue
/// entries. Linux kernel/exit.c:do_exit and kernel/time/hrtimer.c:hrtimer_nanosleep
/// remain the guest lifecycle and deadline authorities.
#[wasm_bindgen]
pub fn lifetime_smoke() -> Result<String, JsValue> {
    wasixv2_host::browser::block_on(async {
        let executor = BrowserExecutor::new(8, 8).map_err(error)?;
        let host = Arc::new(BrowserHost::new(executor.clone()).map_err(error)?);
        for _ in 0..1000 {
            assert!(
                host.sleep(Duration::from_secs(3600))
                    .now_or_never()
                    .is_none()
            );
            let parker = wasixv2_host::browser::Parker::default();
            let observed = parker.sequence();
            parker.notify();
            parker.park(observed);
        }
        let engine = Engine::default();
        let busy = wasixv2::load(
            &engine,
            &wat::parse_str(
                r#"(module (import "wasixv2" "getpid" (func $pid (result i32)))
                (memory (export "memory") 1 2 shared)
                (func (export "_start") (drop (call $pid)) (loop br 0)))"#,
            )
            .map_err(error)?,
        )
        .map_err(error)?;
        for iteration in 0..16 {
            let machine = Arc::new(
                browser::builder(
                    Arc::new(host.services()),
                    executor.clone(),
                    Net::new(NetConfig::default(), None),
                )
                .build(),
            );
            let run = browser::spawn(
                machine.clone(),
                &executor,
                engine.clone(),
                busy.clone(),
                ProcessConfig {
                    args: vec!["lifetime".into()],
                    env: vec![],
                    stdio: Stdio::from_fns(|_| {}, |_| {}),
                },
            )
            .await
            .map_err(error)?;
            for _ in 0..128 {
                assert!(run.wait().now_or_never().is_none());
            }
            let started = host.monotonic();
            match futures_util::future::select(
                run.wait().boxed_local(),
                host.sleep(Duration::from_millis(5)),
            )
            .await
            {
                Either::Left(_) => return Err(error("busy guest unexpectedly exited")),
                Either::Right(((), wait)) => drop(wait),
            }
            assert!(host.monotonic().saturating_sub(started) >= Duration::from_millis(5));
            if iteration % 2 == 0 {
                // Dropping this waiter must preserve the already requested kill.
                drop(run.kill().now_or_never());
            }
            let (one, two) = futures_util::future::join(run.kill(), run.wait()).await;
            assert_eq!(one.map_err(error)?, 137);
            assert_eq!(two.map_err(error)?, 137);
            assert_eq!(run.wait().await.map_err(error)?, 137);
            drop(run);
            drop(machine);
        }
        executor.close();
        Ok(r#"{"cancelledWaits":2048,"timerCancels":1000,"wakeBeforeWait":1000,"deadlineKills":16}"#.into())
    })
}
