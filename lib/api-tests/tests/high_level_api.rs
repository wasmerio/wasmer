static TEST_WAT: &str = r#"
(module
  (table $test-table 2 anyfunc)
  (export "test-table" (table $test-table))
  (export "ret_2" (func $ret_2))
  (export "ret_4" (func $ret_4))
  (elem (;0;) (i32.const 0) $ret_2)
  (func $ret_2 (result i32)
    i32.const 2)
  (func $ret_4 (result i32)
    i32.const 4)
)
"#;

#[test]
fn it_works() {
    use wasmer::{imports, CompiledModule, Func, Module, Table};
    let wasm = wabt::wat2wasm(TEST_WAT).unwrap();
    // TODO: review error messages when `CompiledModule` is not in scope. My hypothesis is that they'll be
    // misleading, if so we may want to do something about it.
    let module = Module::new(wasm).unwrap();
    let import_object = imports! {};
    let instance = module.instantiate(&import_object).unwrap();

    let ret_2: Func<(), i32> = instance.exports_new().get("ret_2").unwrap();
    let ret_4: Func<(), i32> = instance.exports_new().get("ret_4").unwrap();

    assert_eq!(ret_2.call(), Ok(2));
    assert_eq!(ret_4.call(), Ok(4));

    let _test_table: Table = instance.exports_new().get("test-table").unwrap();
    // TODO: when table get is stablized test this
}
