//! Context switching through the public browser facade and task worker profile.
use std::sync::Arc;
use wasixv2::{Engine, Net, NetConfig, ProcessConfig, Stdio, browser};
use wasixv2_host::{
    BrowserHost,
    browser::{BrowserExecutor, block_on, future_to_promise},
};
use wasm_bindgen::prelude::*;

fn error(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

#[wasm_bindgen]
pub fn contexts_smoke() -> Result<String, JsValue> {
    block_on(run(false))
}

#[wasm_bindgen]
pub fn contexts_smoke_jspi() -> js_sys::Promise {
    future_to_promise(async { run(true).await.map(JsValue::from) })
}

async fn run(jspi: bool) -> Result<String, JsValue> {
    let executor = if jspi {
        BrowserExecutor::with_jspi(8, 8)
    } else {
        BrowserExecutor::new(8, 8)
    }
    .map_err(error)?;
    let host = Arc::new(BrowserHost::new(executor.clone()).map_err(error)?);
    let machine = Arc::new(
        browser::builder(
            Arc::new(host.services()),
            executor.clone(),
            Net::new(NetConfig::default(), None),
        )
        .build(),
    );
    let engine = Engine::default();
    let cases = if jspi {
        vec![
            ("contexts", NON_LIFO.to_owned(), 0),
            ("context-return", TERMINAL.replace("CONTEXT_BODY", ""), 129),
            (
                "context-exit",
                TERMINAL.replace("CONTEXT_BODY", "(call $exit (i32.const 42))"),
                42,
            ),
        ]
    } else {
        vec![("contexts-notsup", UNSUPPORTED.to_owned(), 0)]
    };
    for (name, wat, expected) in cases {
        let module = wasixv2::load(&engine, &wat::parse_str(wat).map_err(error)?).map_err(error)?;
        let process = browser::spawn(
            machine.clone(),
            &executor,
            engine.clone(),
            module,
            ProcessConfig {
                args: vec![name.into()],
                env: vec![],
                stdio: Stdio::from_fns(|_| {}, |_| {}),
            },
        )
        .await
        .map_err(error)?;
        assert_eq!(process.wait().await.map_err(error)?, expected, "{name}");
    }
    machine.shutdown().await;
    executor.close();
    Ok(serde_json::json!({"contextSwitching":jspi,"contextDestroy":jspi,"contextReturn":jspi,"contextExit":jspi,"contextNotSup":!jspi}).to_string())
}

// The same non-LIFO stack and ID lifecycle as the native WASIX1 guest regression.
const NON_LIFO: &str = r#"(module
      (import "wasix_32v1" "context_create" (func $create (param i32 i32) (result i32)))
      (import "wasix_32v1" "context_switch" (func $switch (param i64) (result i32)))
      (import "wasix_32v1" "context_destroy" (func $destroy (param i64) (result i32)))
      (import "wasi_snapshot_preview1" "proc_exit" (func $exit (param i32)))
      (memory (export "memory") 1)
      (table 4 funcref)
      (elem (i32.const 1) $a $b $bad)
      (global $phase (mut i32) (i32.const 0))
      (func $bad (param i32))
      (func $check (param $actual i32) (param $expected i32)
        (if (i32.ne (local.get $actual) (local.get $expected)) (then (call $exit (i32.const 90)))))
      (func $a (local $kept i64)
        (local.set $kept (i64.const 0x123456789abcdef))
        (global.set $phase (i32.const 1))
        (call $check (call $switch (i64.load (i32.const 8))) (i32.const 0))
        (call $check (global.get $phase) (i32.const 2))
        (if (i64.ne (local.get $kept) (i64.const 0x123456789abcdef)) (then (call $exit (i32.const 91))))
        (global.set $phase (i32.const 3))
        (call $check (call $switch (i64.const 0)) (i32.const 0))
        ;; Destroy must never resume this suspended body.
        (call $exit (i32.const 92)))
      (func $b
        (call $check (global.get $phase) (i32.const 1))
        (global.set $phase (i32.const 2))
        (call $check (call $switch (i64.load (i32.const 0))) (i32.const 0))
        (call $exit (i32.const 93)))
      (func (export "_start")
        (call $check (call $switch (i64.const 0)) (i32.const 0))
        (call $check (call $create (i32.const 0) (i32.const 1)) (i32.const 0))
        (call $check (call $create (i32.const 8) (i32.const 2)) (i32.const 0))
        (call $check (call $create (i32.const 16) (i32.const 3)) (i32.const 28))
        (call $check (call $create (i32.const 65532) (i32.const 1)) (i32.const 78))
        (call $check (call $switch (i64.load (i32.const 0))) (i32.const 0))
        (call $check (global.get $phase) (i32.const 3))
        (call $check (call $destroy (i64.load (i32.const 8))) (i32.const 0))
        (call $check (call $destroy (i64.load (i32.const 8))) (i32.const 0))
        (call $check (call $switch (i64.load (i32.const 8))) (i32.const 28))
        (call $check (call $destroy (i64.const 0)) (i32.const 28))
        (call $check (call $destroy (i64.load (i32.const 0))) (i32.const 0))
        (call $exit (i32.const 0))))"#;
const TERMINAL: &str = r#"(module
      (import "wasix_32v1" "context_create" (func $create (param i32 i32) (result i32)))
      (import "wasix_32v1" "context_switch" (func $switch (param i64) (result i32)))
      (import "wasi_snapshot_preview1" "proc_exit" (func $exit (param i32)))
      (memory (export "memory") 1)
      (table 2 funcref)
      (elem (i32.const 1) $context)
      (func $context CONTEXT_BODY)
      (func (export "_start")
        (drop (call $create (i32.const 0) (i32.const 1)))
        (drop (call $switch (i64.load (i32.const 0))))
        (call $exit (i32.const 94))))"#;
const UNSUPPORTED: &str = r#"(module
  (import "wasix_32v1" "context_create" (func $create (param i32 i32) (result i32)))
  (import "wasix_32v1" "context_switch" (func $switch (param i64) (result i32)))
  (import "wasix_32v1" "context_destroy" (func $destroy (param i64) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $exit (param i32)))
  (memory (export "memory") 1)
  (table 2 funcref) (elem (i32.const 1) $context)
  (func $context (call $exit (i32.const 91)))
  (func $check (param $actual i32)
    (if (i32.ne (local.get $actual) (i32.const 58)) (then (call $exit (i32.const 90)))))
  (func (export "_start")
    (call $check (call $create (i32.const 0) (i32.const 1)))
    (call $check (call $switch (i64.const 0)))
    (call $check (call $destroy (i64.const 1)))
    (call $exit (i32.const 0))))"#;
