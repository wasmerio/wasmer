mod runtime_core_tests;

use runtime_core_tests::{get_compiler, wat2wasm};
use wasmer_runtime_core::{compile_with, imports};

#[test]
fn exception_handling_works() {
    const MODULE: &str = r#"
(module
  (func (export "throw_trap")
    unreachable
  ))
"#;

    let wasm_binary = wat2wasm(MODULE.as_bytes()).expect("WAST not valid or malformed");
    let module = compile_with(&wasm_binary, &get_compiler()).unwrap();

    let imports = imports! {};
    for _ in 0..2 {
        let instance = module.instantiate(&imports).unwrap();
        assert!(instance.call("throw_trap", &[]).is_err());
    }
}
