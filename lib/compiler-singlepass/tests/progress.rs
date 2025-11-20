#![cfg(all(
    feature = "std",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]

use std::sync::{Arc, Mutex};

use wasmer_compiler::EngineBuilder;
use wasmer_types::{CompilationProgressCallback, CompileError, UserAbort};

const SIMPLE_WAT: &str = r#"(module
  (import "env" "div" (func $div (param i32 i32) (result i32)))
  (func (export "add") (param i32 i32) (result i32)
    local.get 0
    local.get 1
    i32.add)
  (func (export "sub") (param i32 i32) (result i32)
    local.get 0
    local.get 1
    i32.sub)
)"#;

fn singlepass_engine() -> wasmer_compiler::Engine {
    let compiler = wasmer_compiler_singlepass::Singlepass::new();
    EngineBuilder::new(compiler).engine()
}

fn wasm_bytes() -> Vec<u8> {
    wat::parse_str(SIMPLE_WAT).expect("valid wat")
}

#[test]
fn singlepass_reports_progress_steps() {
    let engine = singlepass_engine();
    let wasm = wasm_bytes();

    let events = Arc::new(Mutex::new(Vec::new()));
    let cb = CompilationProgressCallback::new({
        let events = events.clone();
        move |progress| {
            events.lock().unwrap().push(progress);
            Ok(())
        }
    });

    engine
        .compile_with_progress(&wasm, Some(cb))
        .expect("compilation succeeds");

    let events = events.lock().unwrap();
    assert!(
        !events.is_empty(),
        "expected at least one progress notification"
    );
    let last = events.last().unwrap();
    assert_eq!(last.phase_step_count(), Some(4));
    assert_eq!(last.phase_step(), Some(4));
}

#[test]
fn singlepass_progress_can_abort() {
    let engine = singlepass_engine();
    let wasm = wasm_bytes();

    let cb = CompilationProgressCallback::new(|_| Err(UserAbort::new("abort")));
    match engine.compile_with_progress(&wasm, Some(cb)) {
        Err(CompileError::Aborted(e)) => assert_eq!(e.reason(), "abort"),
        other => panic!("expected CompileError::Aborted, got {:?}", other),
    }
}
