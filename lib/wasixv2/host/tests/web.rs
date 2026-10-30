//! `WebHost` on a real JavaScript event loop. Run under node with
//! `CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner cargo test -p wasixv2-host
//! --target wasm32-unknown-unknown --no-default-features --features web`; the runner must match
//! the locked `wasm-bindgen` version.
#![cfg(all(target_arch = "wasm32", feature = "web"))]

use std::{cell::Cell, rc::Rc, time::Duration};

use wasixv2_host::{Host, WebHost};
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn clocks_are_plausible() {
    let host = WebHost::new();
    // After 2020-01-01.
    assert!(host.realtime() > Duration::from_secs(1_577_836_800));
    let before = host.monotonic();
    assert!(host.monotonic() >= before);
}

#[wasm_bindgen_test]
fn random_fills_buffers_beyond_one_chunk() {
    let mut buf = vec![0u8; 100_000];
    WebHost::new().random(&mut buf);
    assert!(buf[..65536].iter().any(|&b| b != 0));
    assert!(buf[65536..].iter().any(|&b| b != 0));
}

#[wasm_bindgen_test]
async fn sleep_waits_at_least_the_duration() {
    let host = WebHost::new();
    let start = host.monotonic();
    host.sleep(Duration::from_millis(20)).await;
    assert!(host.monotonic() - start >= Duration::from_millis(20));
}

#[wasm_bindgen_test]
async fn spawn_local_runs_in_the_background() {
    let host = WebHost::new();
    let ran = Rc::new(Cell::new(false));
    let flag = ran.clone();
    host.executor()
        .spawn_local(Box::pin(async move { flag.set(true) }));
    assert!(!ran.get());
    host.sleep(Duration::from_millis(1)).await;
    assert!(ran.get());
    assert_eq!(host.executor().run_blocking(|| 6 * 7).await, 42);
}

#[wasm_bindgen_test]
fn canceled_sleeps_release_their_rust_callback_roots() {
    use std::task::{Context, Waker};
    let host = WebHost::new();
    let before = wasm_bindgen::externref_heap_live_count();
    for _ in 0..10 {
        let mut sleep = host.sleep(Duration::from_secs(3600));
        assert!(
            sleep
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        drop(sleep);
    }
    assert_eq!(wasm_bindgen::externref_heap_live_count(), before);
}
