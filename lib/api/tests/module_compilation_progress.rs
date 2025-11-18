#![cfg(any(feature = "cranelift", feature = "llvm", feature = "singlepass"))]

use std::sync::{Arc, Mutex};

use wasmer::{CompileError, Engine, ProgressEngineExt as _};
use wasmer_types::{CompilationProgress, UserAbort};

#[cfg(feature = "singlepass")]
#[test]
fn test_module_compilation_progress_singlepass() {
    let compiler = wasmer::sys::Singlepass::default();
    let engine: wasmer::Engine = wasmer::sys::EngineBuilder::new(compiler).engine().into();
    test_module_compilation_progress(engine);
}

#[cfg(feature = "singlepass")]
#[test]
fn test_module_compilation_abort_singlepass() {
    let compiler = wasmer::sys::Cranelift::default();
    let engine: wasmer::Engine = wasmer::sys::EngineBuilder::new(compiler).engine().into();
    test_module_compilation_abort(engine);
}

#[cfg(feature = "cranelift")]
#[test]
fn test_module_compilation_progress_cranelift() {
    let compiler = wasmer::sys::Cranelift::default();
    let engine: wasmer::Engine = wasmer::sys::EngineBuilder::new(compiler).engine().into();
    test_module_compilation_progress(engine);
}

#[cfg(feature = "cranelift")]
#[test]
fn test_module_compilation_abort_cranelift() {
    let compiler = wasmer::sys::Cranelift::default();
    let engine: wasmer::Engine = wasmer::sys::EngineBuilder::new(compiler).engine().into();
    test_module_compilation_abort(engine);
}

#[cfg(feature = "llvm")]
#[test]
fn test_module_compilation_progress_llvm() {
    let compiler = wasmer::sys::LLVM::default();
    let engine: wasmer::Engine = wasmer::sys::EngineBuilder::new(compiler).engine().into();
    test_module_compilation_progress(engine);
}

#[cfg(feature = "llvm")]
#[test]
fn test_module_compilation_abort_llvm() {
    let compiler = wasmer::sys::LLVM::default();
    let engine: wasmer::Engine = wasmer::sys::EngineBuilder::new(compiler).engine().into();
    test_module_compilation_abort(engine);
}

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
) "#;

fn test_module_compilation_progress(engine: Engine) {
    let items = Arc::new(Mutex::new(Vec::<CompilationProgress>::new()));

    let cb = wasmer_types::CompilationProgressCallback::new({
        let items = items.clone();
        move |p| {
            items.lock().unwrap().push(p);
            Ok(())
        }
    });
    let _module = engine
        .new_module_with_progress(SIMPLE_WAT.as_bytes(), cb)
        .unwrap();

    let last = items
        .lock()
        .unwrap()
        .last()
        .expect("expected at least one progress item")
        .clone();

    // 4 total steps:
    // - 2 functions
    // - 1 trampoline for exports (both share same signature)
    // - 1 trampoline for imported function
    assert_eq!(last.phase_step_count(), Some(4));
    assert_eq!(last.phase_step(), Some(4));
}

fn test_module_compilation_abort(engine: Engine) {
    let reason = "my reason";
    let cb = wasmer_types::CompilationProgressCallback::new(move |_p| Err(UserAbort::new(reason)));
    let err = engine
        .new_module_with_progress(SIMPLE_WAT.as_bytes(), cb)
        .expect_err("should fail");

    match err {
        CompileError::Aborted(e) => {
            assert_eq!(e.reason(), reason)
        }
        other => {
            panic!("expected CompileError::Aborted, got {:?}", other);
        }
    }
}
