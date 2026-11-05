//! Historical HTTP imports must link even without a provider: capability failure is a WAI
//! expected<string>, not a missing-import failure or a fabricated successful response.

#![recursion_limit = "256"]

use std::sync::Arc;
use wasixv2_wasix1::{
    Wasix1,
    http::{HttpProvider, Request, Response},
};

fn host() -> Arc<dyn wasixv2::Host> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Arc::new(wasixv2::TokioHost::default())
    }
    #[cfg(target_arch = "wasm32")]
    {
        Arc::new(wasixv2::WebHost::new())
    }
}

struct Mock;
impl HttpProvider for Mock {
    fn send(&self, request: Request) -> wasixv2::LocalFuture<'_, Result<Response, String>> {
        Box::pin(async move {
            // Exercise completion after the shim registered its signal wait, including
            // wake-before-park on the blocking JS worker. No timer/Promise on that worker.
            let mut first = true;
            std::future::poll_fn(|cx| {
                if std::mem::take(&mut first) {
                    cx.waker().wake_by_ref();
                    std::task::Poll::Pending
                } else {
                    std::task::Poll::Ready(())
                }
            })
            .await;
            assert_eq!(request.url, "https://fixture.test/");
            if request.method == "POST" {
                return Err("CORS/fetch failure".to_owned());
            }
            Ok(Response {
                status: 201,
                headers: vec![("x-test".to_owned(), vec![0, 128, 255])],
                body: b"hello".to_vec(),
                redirect_urls: Some(vec!["https://fixture.test/".to_owned()]),
            })
        })
    }
}

async fn run(wat: &str, provider: Arc<dyn HttpProvider>) -> i32 {
    let mut machine = wasixv2::MachineBuilder::new(host())
        .wasix1_http_provider(provider)
        .build();
    // Root assembly reconstructs the ABI registry; the configured guest capability
    // must survive it, just as exec retains process resources (fs/exec.c:begin_new_exec).
    let layer = machine
        .commands_layer([(
            "http".to_owned(),
            wasixv2::OwnedBuffer::from_bytes(&b"unused"[..]),
        )])
        .unwrap()
        .unwrap();
    machine.mount_root_layers([layer]).await.unwrap();
    machine
        .run(
            &wat::parse_str(wat).unwrap(),
            wasixv2::Stdio::from_fns(|_| {}, |_| {}),
        )
        .await
        .unwrap()
}

async fn run_with_task(
    wat: &str,
    provider: Arc<dyn HttpProvider>,
    on_task: impl FnOnce(&Arc<wasixv2_kernel::Kernel>, wasixv2_syscalls::TaskId),
) -> i32 {
    let host = host();
    let root = Arc::new(wasixv2_vfs_mem::MemFs::new(
        Arc::new(wasixv2_kernel::HostClock(host.clone())),
        0o777,
        1,
        1,
    ));
    let mut abis = wasixv2_exec::Abis::new();
    abis.push(Arc::new(Wasix1::new().with_http_provider(provider)));
    let abis = Arc::new(abis);
    let kernel =
        Arc::new(wasixv2_kernel::Kernel::new(host.clone(), root).with_binfmt(abis.clone()));
    let task = kernel
        .spawn(wasixv2::ProcessConfig {
            args: vec!["http".to_owned()],
            env: Vec::new(),
            stdio: wasixv2::Stdio::from_fns(|_| {}, |_| {}),
        })
        .unwrap();
    on_task(&kernel, task);
    let store = wasmer::Store::default();
    let module = wasixv2_exec::load(store.engine(), &wat::parse_str(wat).unwrap()).unwrap();
    wasixv2_wasix1::imports::validate(&module).unwrap();
    wasixv2_exec::run_with_abis(store, &module, kernel, task, host, abis)
        .await
        .unwrap()
}

#[cfg(not(target_arch = "wasm32"))]
#[tokio::test]
async fn a_pending_provider_is_canceled_when_the_guest_is_killed() {
    pending_signal(wasixv2_abi::Signal::SIGKILL, 137, false).await;
}

#[cfg(not(target_arch = "wasm32"))]
#[tokio::test]
async fn a_handled_signal_interrupts_http_without_replaying_it() {
    pending_signal(wasixv2_abi::Signal::SIGUSR1, 0, true).await;
}

#[cfg(not(target_arch = "wasm32"))]
async fn pending_signal(signal: wasixv2_abi::Signal, expected: i32, handled: bool) {
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };
    struct Pending {
        started: Mutex<mpsc::Sender<()>>,
        dropped: Arc<AtomicUsize>,
        requests: AtomicUsize,
    }
    struct Guard(Arc<AtomicUsize>);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl HttpProvider for Pending {
        fn send(&self, _: Request) -> wasixv2::LocalFuture<'_, Result<Response, String>> {
            self.requests.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                let _guard = Guard(self.dropped.clone());
                self.started.lock().unwrap().send(()).unwrap();
                std::future::pending().await
            })
        }
    }
    let (tx, rx) = mpsc::channel();
    let provider = Arc::new(Pending {
        started: Mutex::new(tx),
        dropped: Arc::default(),
        requests: AtomicUsize::new(0),
    });
    let setup = if handled {
        "(call $callback (i32.const 1000) (i32.const 13))"
    } else {
        ""
    };
    let finish = if handled {
        r#"(if (i32.ne (i32.load (i32.const 300)) (i32.const 1)) (then unreachable))
           (if (i32.ne (i32.load8_u (i32.const 512)) (i32.const 1)) (then unreachable))
           (if (i32.ne (i32.load (i32.const 520)) (i32.const 24)) (then unreachable))
           (call $exit (i32.const 0))"#
    } else {
        "unreachable"
    };
    let source = format!(
        r#"(module
        (import "wasix_http_client_v1" "client::new" (func $new (param i32)))
        (import "wasix_http_client_v1" "client::send" (func $send (param i32 i32)))
        (import "wasi_snapshot_preview1" "sched_yield" (func $yield (result i32)))
        (import "wasix_32v1" "callback_signal" (func $callback (param i32 i32)))
        (import "wasix_32v1" "proc_exit2" (func $exit (param i32)))
        (memory (export "memory") 1)
        (data (i32.const 1000) "__wasm_signal")
        {ALLOCATOR}
        (func (export "__wasm_signal") (param i32)
          (i32.store (i32.const 300) (i32.const 1)))
        (func (export "_start")
          {setup}
          (call $new (i32.const 0))
          (i32.store (i32.const 128) (i32.load (i32.const 4)))
          (call $send (i32.const 128) (i32.const 512))
          {finish}))"#
    );
    let mut killer = None;
    let status = run_with_task(&source, provider.clone(), |kernel, task| {
        let kernel = kernel.clone();
        killer = Some(std::thread::spawn(move || {
            rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            kernel.kill(task, signal).unwrap();
        }));
    })
    .await;
    killer.unwrap().join().unwrap();
    assert_eq!(status, expected);
    assert_eq!(provider.requests.load(Ordering::SeqCst), 1);
    assert_eq!(provider.dropped.load(Ordering::SeqCst), 1);
}

/// A canonical bump allocator that reenters a host import: no store/table mutex may be held
/// while the compatibility encoder calls it, including on the blocking JS lane.
const ALLOCATOR: &str = r#"
    (global $next (mut i32) (i32.const 4096))
    (func (export "canonical_abi_realloc")
      (param i32 i32) (param $align i32) (param $len i32) (result i32)
      (local $ptr i32)
      (drop (call $yield))
      (local.set $ptr (i32.and
        (i32.add (global.get $next) (i32.sub (local.get $align) (i32.const 1)))
        (i32.sub (i32.const 0) (local.get $align))))
      (global.set $next (i32.add (local.get $ptr) (local.get $len)))
      (local.get $ptr))
"#;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), tokio::test)]
async fn mock_response_errors_and_resource_drop_use_the_historical_layout() {
    let source = format!(
        r#"(module
        (import "wasix_http_client_v1" "client::new" (func $new (param i32)))
        (import "wasix_http_client_v1" "client::send" (func $send (param i32 i32)))
        (import "canonical_abi" "resource_drop_client" (func $drop (param i32)))
        (import "wasi_snapshot_preview1" "sched_yield" (func $yield (result i32)))
        (import "wasi_snapshot_preview1" "proc_exit" (func $exit (param i32)))
        (memory (export "memory") 1)
        (data (i32.const 256) "https://fixture.test/")
        {ALLOCATOR}
        (func (export "_start")
          (call $new (i32.const 0))
          (if (i32.load8_u (i32.const 0)) (then unreachable))
          (i32.store (i32.const 128) (i32.load (i32.const 4)))
          (i32.store (i32.const 132) (i32.const 256))
          (i32.store (i32.const 136) (i32.const 21))
          (call $send (i32.const 128) (i32.const 512))
          (if (i32.load8_u (i32.const 512)) (then unreachable))
          (if (i32.ne (i32.load16_u (i32.const 516)) (i32.const 201)) (then unreachable))
          (if (i32.ne (i32.load (i32.const 524)) (i32.const 1)) (then unreachable))
          ;; One header record, binary value 00 80 ff (zero-extended load16 at value+1).
          (if (i32.ne (i32.load (i32.add (i32.load (i32.const 520)) (i32.const 4))) (i32.const 6)) (then unreachable))
          (if (i32.ne (i32.load16_u offset=1 (i32.load offset=8 (i32.load (i32.const 520)))) (i32.const 65408)) (then unreachable))
          (if (i32.load8_u (i32.const 528)) (then unreachable))
          (if (i32.ne (i32.load (i32.const 536)) (i32.const 5)) (then unreachable))
          (if (i32.ne (i32.load (i32.load (i32.const 532))) (i32.const 1819043176)) (then unreachable))
          (if (i32.ne (i32.load8_u (i32.const 540)) (i32.const 1)) (then unreachable))
          (if (i32.ne (i32.load (i32.const 548)) (i32.const 1)) (then unreachable))
          ;; POST asks the mock to fail; the expected<string> error occupies offsets 0/4/8.
          (i32.store8 (i32.const 140) (i32.const 2))
          (call $send (i32.const 128) (i32.const 512))
          (if (i32.ne (i32.load8_u (i32.const 512)) (i32.const 1)) (then unreachable))
          (if (i32.ne (i32.load (i32.const 520)) (i32.const 18)) (then unreachable))
          (if (i32.ne (i32.load (i32.load (i32.const 516))) (i32.const 1397903171)) (then unreachable))
          (call $drop (i32.load (i32.const 128)))
          (call $exit (i32.const 0))))"#
    );
    assert_eq!(run(&source, Arc::new(Mock)).await, 0);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), tokio::test)]
async fn no_provider_returns_a_canonical_error() {
    let machine = wasixv2::Machine::with_host(host());
    let wasm = wat::parse_str(r#"(module
        (import "wasix_http_client_v1" "client::new" (func $new (param i32)))
        (import "wasi_snapshot_preview1" "proc_exit" (func $exit (param i32)))
        (memory (export "memory") 1)
        (func (export "canonical_abi_realloc") (param i32 i32 i32 i32) (result i32) i32.const 1024)
        (func (export "_start")
            (call $new (i32.const 0))
            (if (i32.ne (i32.load8_u (i32.const 0)) (i32.const 1)) (then unreachable))
            (if (i32.ne (i32.load (i32.const 8)) (i32.const 24)) (then unreachable))
            ;; "No http client available" in the guest's allocated error string.
            (if (i32.ne (i32.load (i32.load (i32.const 4))) (i32.const 1746956110)) (then unreachable))
            (call $exit (i32.const 0))))"#).unwrap();
    assert_eq!(
        machine
            .run(&wasm, wasixv2::Stdio::from_fns(|_| {}, |_| {}))
            .await
            .unwrap(),
        0
    );
}
