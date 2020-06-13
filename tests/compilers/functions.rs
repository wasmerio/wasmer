use crate::utils::get_store;
use anyhow::Result;

use wasmer::*;

#[test]
fn native_function_works_for_wasm() -> Result<()> {
    let store = get_store();
    let wat = r#"(module
        (func $multiply (import "env" "multiply") (param i32 i32) (result i32))
        (func (export "add") (param i32 i32) (result i32)
           (i32.add (local.get 0)
                    (local.get 1)))
        (func (export "double_then_add") (param i32 i32) (result i32)
           (i32.add (call $multiply (local.get 0) (i32.const 2))
                    (call $multiply (local.get 1) (i32.const 2))))
)"#;
    let module = Module::new(&store, wat).unwrap();

    let import_object = imports! {
        "env" => {
            "multiply" => Function::new(&store, |a: i32, b: i32| a * b),
        },
    };

    let instance = Instance::new(&module, &import_object)?;

    let f: NativeFunc<(i32, i32), i32> = instance.exports.get_native_function("add")?;
    let result = f.call(4, 6)?;
    assert_eq!(result, 10);

    let dyn_f: &Function = instance.exports.get("double_then_add")?;
    let dyn_result = dyn_f.call(&[Val::I32(4), Val::I32(6)])?;
    assert_eq!(dyn_result[0], Val::I32(20));

    let f: NativeFunc<(i32, i32), i32> = dyn_f.native().unwrap();

    let result = f.call(4, 6)?;
    assert_eq!(result, 20);
    Ok(())
}

fn dynamic_raw_call_no_env() -> anyhow::Result<()> {
    let store = get_store();
    let reverse_duplicate = wasmer::Function::new_dynamic(
        &store,
        &wasmer::FunctionType::new(
            vec![
                wasmer::ValType::I32,
                wasmer::ValType::I64,
                wasmer::ValType::F32,
                wasmer::ValType::F64,
            ],
            vec![wasmer::ValType::F64],
        ),
        |values| {
            Ok(vec![
                Value::F64(values[3].unwrap_f64() * 2.0),
                Value::F32(values[2].unwrap_f32() * 2.0),
                Value::I64(values[2].unwrap_i64() * 2.0),
                Value::I32(values[2].unwrap_i32() * 2.0),
            ])
        },
    );
    let reverse_duplicate_native: NativeFunc<(i32, i64, f32, f64), (f64, f32, i64, i32)> =
        reverse_duplicate.native().unwrap();
    let result = reverse_duplicate_native.call(1, 3, 5.0, 7.0)?;
    assert_eq!(result, (14.0, 10.0, 6, 2));
    Ok(())
}
