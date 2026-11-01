//! Checked table lookup must not manufacture type metadata for passive function refs.

#![cfg(all(target_arch = "wasm32", feature = "js"))]

use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
use wasmer::{Function, Imports, Instance, Module, Store, Type, Value};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn unchecked_lookup_does_not_poison_an_unknown_function_signature() {
    let mut store = Store::default();
    let source = wat::parse_str(
        r#"(module
        (table (export "table") 1 funcref)
        (func $target (param i64) (result i32) i32.const 7)
        (elem $functions func $target)
        (func (export "install")
            i32.const 0 i32.const 0 i32.const 1 table.init $functions))"#,
    )
    .unwrap();
    let module = Module::new(&store, source).unwrap();
    let instance = Instance::new(&mut store, &module, &Imports::new()).unwrap();
    instance
        .exports
        .get_function("install")
        .unwrap()
        .call(&mut store, &[])
        .unwrap();
    let table = instance.exports.get_table("table").unwrap();
    assert!(table.get_typed_function(&mut store, 0).is_none());
    assert!(matches!(
        table.get(&mut store, 0),
        Some(Value::FuncRef(Some(_)))
    ));
    assert!(table.get_typed_function(&mut store, 0).is_none());

    let known = Function::new_typed(&mut store, |_: i64| -> i32 { 9 });
    table
        .set(&mut store, 0, Value::FuncRef(Some(known)))
        .unwrap();
    let function = table.get_typed_function(&mut store, 0).unwrap();
    assert_eq!(function.ty(&store).params(), &[Type::I64]);
    assert_eq!(function.ty(&store).results(), &[Type::I32]);
}
