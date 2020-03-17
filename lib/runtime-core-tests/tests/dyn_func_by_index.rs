use wasmer_runtime_core::{
    compile_with, imports,
    types::FuncSig,
    types::{Type, Value},
};
use wasmer_runtime_core_tests::{get_compiler, wat2wasm};

#[test]
fn dyn_func_by_index() {
    const MODULE: &str = r#"
(module
  (func (export "foo") (param i32) (result i32)
    get_local 0
    i32.const 1
    i32.add))
"#;

    let wasm_binary = wat2wasm(MODULE.as_bytes()).expect("WAST not valid or malformed");
    let module = compile_with(&wasm_binary, &get_compiler()).unwrap();
    let import_object = imports! {};
    let instance = module.instantiate(&import_object).unwrap();

    let func = instance.dyn_func_by_index(0).unwrap();

    assert_eq!(
        *func.signature(),
        FuncSig::new(vec![Type::I32], vec![Type::I32])
    );

    let results = func.call(&[Value::I32(1)]);

    assert_eq!(results, Ok(vec![Value::I32(2)]));
}
