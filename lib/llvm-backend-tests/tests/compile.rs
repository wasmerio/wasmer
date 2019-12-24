use wasmer_llvm_backend::{InkwellModule, LLVMBackendConfig, LLVMCallbacks};
use wasmer_llvm_backend_tests::{get_compiler, wat2wasm};
use wasmer_runtime::{imports, CompilerConfig};
use wasmer_runtime_core::{backend::BackendCompilerConfig, compile_with, compile_with_config};

use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn crash_return_with_float_on_stack() {
    const MODULE: &str = r#"
(module
  (type (func))
  (type (func (param f64) (result f64)))
  (func $_start (type 0))
  (func $fmod (type 1) (param f64) (result f64)
    local.get 0
    f64.const 0x0p+0
    f64.mul
    return))
"#;
    let wasm_binary = wat2wasm(MODULE.as_bytes()).expect("WAST not valid or malformed");
    let module = compile_with(&wasm_binary, &get_compiler()).unwrap();
    module.instantiate(&imports! {}).unwrap();
}

#[derive(Debug, Default)]
pub struct RecordPreOptIR {
    preopt_ir: String,
}

impl LLVMCallbacks for RecordPreOptIR {
    fn preopt_ir_callback(&mut self, module: &InkwellModule) {
        self.preopt_ir = module.print_to_string().to_string();
    }
}

#[test]
fn crash_select_with_mismatched_pending() {
    const WAT: &str = r#"
 (module
  (func (param f64) (result f64)
    f64.const 0x0p+0
    local.get 0
    f64.add
    f64.const 0x0p+0
    i32.const 0
    select))
"#;
    let record_pre_opt_ir = Rc::new(RefCell::new(RecordPreOptIR::default()));
    let compiler_config = CompilerConfig {
        backend_specific_config: Some(BackendCompilerConfig(Box::new(LLVMBackendConfig {
            callbacks: Some(record_pre_opt_ir.clone()),
        }))),
        ..Default::default()
    };
    let wasm_binary = wat2wasm(WAT.as_bytes()).expect("WAST not valid or malformed");
    let module = compile_with_config(&wasm_binary, &get_compiler(), compiler_config).unwrap();
    module.instantiate(&imports! {}).unwrap();
    const LLVM: &str = r#"
  %s3 = fadd double 0.000000e+00, %s2
  %nan = fcmp uno double %s3, 0.000000e+00
  %2 = select i1 %nan, double 0x7FF8000000000000, double %s3
  %s5 = select i1 false, double %2, double 0.000000e+00
  br label %return
"#;
    assert!(&record_pre_opt_ir.borrow().preopt_ir.contains(LLVM));
}
