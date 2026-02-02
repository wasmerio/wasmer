use anyhow::Result;
use std::sync::{Arc, Mutex};

use wasmer::{sys::NativeEngineExt, wat2wasm};
use wasmer_types::{CompilationProgressCallback, CompileError, UserAbort};

use crate::Compiler;

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

fn wasm_bytes() -> Vec<u8> {
    wat2wasm(SIMPLE_WAT.as_bytes()).expect("valid wat").to_vec()
}
#[compiler_test(issues)]
fn reports_progress_steps(mut config: crate::Config) -> Result<()> {
    let engine = config.store();
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
        .engine()
        .new_module_with_progress(&wasm, cb)
        .expect("compilation succeeds");

    let events = events.lock().unwrap();
    assert!(
        !events.is_empty(),
        "expected at least one progress notification"
    );
    let last = events.last().unwrap();
    // LLVM/Cranelift compiler uses bitcode size for the total.
    if matches!(config.compiler, Compiler::LLVM | Compiler::Cranelift) {
        assert_eq!(last.phase_step_count(), Some(2014));
        assert_eq!(last.phase_step(), Some(2014));
    } else {
        assert_eq!(last.phase_step_count(), Some(4));
        assert_eq!(last.phase_step(), Some(4));
    }
    Ok(())
}

#[compiler_test(issues)]
fn progress_can_abort(mut config: crate::Config) -> Result<()> {
    let engine = config.store();
    let wasm = wasm_bytes();

    let cb = CompilationProgressCallback::new(|_| Err(UserAbort::new("abort")));
    match engine.engine().new_module_with_progress(&wasm, cb) {
        Err(CompileError::Aborted(e)) => assert_eq!(e.reason(), "abort"),
        other => panic!("expected CompileError::Aborted, got {:?}", other),
    }
    Ok(())
}
