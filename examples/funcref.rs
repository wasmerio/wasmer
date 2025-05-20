use std::error::Error;
use wasmer::{imports, Instance, Module, Store, Table, TableType, Type, Value};

fn main() -> Result<(), Box<dyn Error>> {
    // 1) Create a default Wasmer Store.
    let mut store = Store::default();

    // 2) Our first WAT module ("module A") that exports `add_one`.
    let module_a_wat = r#"(module
  (type $t0 (func (param i32) (result i32)))
  (func $add_one (type $t0) (param $x i32) (result i32)
    local.get $x
    i32.const 1
    i32.add)
  (export "add_one" (func $add_one))
)"#;

    // 3) Our second WAT module ("module B") that imports the table and uses `call_ref`.
    let module_b_wat = r#"(module
  (type $t0 (func (param i32) (result i32)))
  (import "env" "table" (table 1 funcref))
  (func $call_func (param i32) (result i32)
    (local.get 0)
    (call_indirect (type $t0)
      (i32.const 0)
    )
  )
  (export "call_func" (func $call_func))
)"#;

    // 4) Compile module A.
    let module_a = Module::new(&store, module_a_wat)?;
    // 5) Instantiate module A (no imports needed).
    let instance_a = Instance::new(&mut store, &module_a, &imports! {})?;

    // 6) Get the exported function `add_one`.
    let add_one_func = instance_a.exports.get_function("add_one")?;

    // 7) Create a Table for `function`.
    //    Initial size = 1, Max size = 1 (optional).
    let table_type = TableType::new(Type::FuncRef, 1, Some(1)); // anyref => ExternRef in Wasmer
    let table = Table::new(&mut store, table_type, Value::FuncRef(None))?;

    // 8) Place the `add_one_func` into table[0] as an ExternRef.
    //    Note: In older Wasmer versions, you might need `Value::FuncRef`.
    //    But for the new `anyref`, we store it as `Value::ExternRef(Some(...))`.
    table.set(&mut store, 0, Value::FuncRef(Some(add_one_func.clone())))?;

    // 9) Compile module B (which imports the table).
    let module_b = Module::new(&store, module_b_wat)?;

    // 10) Create an ImportObject providing the table to module B.
    let import_object = imports! {
        "env" => {
            "table" => table.clone(),
        }
    };

    // 11) Instantiate module B with that import.
    let instance_b = Instance::new(&mut store, &module_b, &import_object)?;

    // 12) Get the exported `call_func` from module B.
    let call_func = instance_b.exports.get_function("call_func")?;

    // 13) Call `call_func(41)`, which should end up calling `add_one(41)`.
    let results = call_func.call(&mut store, &[Value::I32(41)])?;

    // 14) Print the result (should be 42).
    println!(
        "Result from module B calling module A's `add_one(41)`: {}",
        results[0].i32().unwrap()
    );

    Ok(())
}
