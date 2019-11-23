use wasmer_llvm_backend_tests::{get_compiler, wat2wasm};
use wasmer_runtime::imports;
use wasmer_runtime_core::compile_with;

#[test]
fn crash_return_with_float_on_stack() {
    const MODULE: &str = r#"
(module
  (type (;0;) (func))
  (type (;1;) (func (param f64) (result f64)))
  (func $_start (type 0))
  (func $fmod (type 1) (param f64) (result f64)
    local.get 0
    f64.const 0x0p+0 (;=0;)
    f64.mul
    return)
)
"#;
    let wasm_binary = wat2wasm(MODULE.as_bytes()).expect("WAST not valid or malformed");
    let module = compile_with(&wasm_binary, &get_compiler()).unwrap();
    let instance = module.instantiate(&imports! {}).unwrap();
}
