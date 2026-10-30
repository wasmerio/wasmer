//! Historical HTTP imports must link even without a provider: capability failure is a WAI
//! expected<string>, not a missing-import failure or a fabricated successful response.

#![recursion_limit = "256"]

use std::sync::Arc;
use wasixv2_wasix1::{
    Wasix1,
    http::{HttpProvider, Request, Response},
};

struct Mock;
impl HttpProvider for Mock {
    fn send(&self, request: Request) -> wasixv2::LocalFuture<'_, Result<Response, String>> {
        Box::pin(async move {
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
    let host: Arc<dyn wasixv2::Host> = Arc::new(wasixv2::TokioHost::default());
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
    let store = wasmer::Store::default();
    let module = wasixv2_exec::load(store.engine(), &wat::parse_str(wat).unwrap()).unwrap();
    wasixv2_wasix1::imports::validate(&module).unwrap();
    wasixv2_exec::run_with_abis(store, &module, kernel, task, host, abis)
        .await
        .unwrap()
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

#[tokio::test]
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

#[tokio::test]
async fn no_provider_returns_a_canonical_error() {
    let machine = wasixv2::Machine::new();
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
