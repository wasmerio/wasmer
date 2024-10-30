use wasmer::{
    imports, wat2wasm, Function, Instance, Module, Store, TableType, Type, TypedFunction, Value,
};

/// A function we'll call through a table.
fn host_callback(arg1: i32, arg2: i32) -> i32 {
    arg1 + arg2
}

fn main() -> anyhow::Result<()> {
    let wasm_bytes = wat2wasm(
        r#"
(module
  ;; All our callbacks will take 2 i32s and return an i32.
  ;; Wasm tables are not limited to 1 type of function, but the code using the
  ;; table must have code to handle the type it finds.
  (type $callback_t (func (param i32 i32) (result i32)))

  ;; We'll call a callback by passing a table index as an i32 and then the two
  ;; arguments that the function expects.
  (type $call_callback_t (func (param i32 i32 i32) (result i32)))

  ;; Our table of functions that's exactly size 3 (min 3, max 3).
  (table $t1 3 6 funcref)

  ;; Call the function at the given index with the two supplied arguments.
  (func $call_callback (type $call_callback_t) (param $idx i32)
                                               (param $arg1 i32) (param $arg2 i32)
                                               (result i32)
    (call_indirect (type $callback_t) 
                   (local.get $arg1) (local.get $arg2)
                   (local.get $idx)))

  ;; A default function that we'll pad the table with.
  ;; This function doubles both its inputs and then sums them.
  (func $default_fn (type $callback_t) (param $a i32) (param $b i32) (result i32)
     (i32.add 
       (i32.mul (local.get $a) (i32.const 2))
       (i32.mul (local.get $b) (i32.const 2))))

  ;; Fill our table with the default function.
  (elem $t1 (i32.const 0) $default_fn $default_fn $default_fn)

  ;; Export things for the host to call.
  (export "call_callback" (func $call_callback))
  (export "__indirect_function_table" (table $t1)))
"#
        .as_bytes(),
    )?;

    // Create a Store.
    let mut store = Store::default();
    // Then compile our Wasm.
    let module = Module::new(&store, wasm_bytes)?;
    let import_object = imports! {};
    // And instantiate it with no imports.
    let instance = Instance::new(&mut store, &module, &import_object)?;

    // We get our function that calls (i32, i32) -> i32 functions via table.
    // The first argument is the table index and the next 2 are the 2 arguments
    // to be passed to the function found in the table.
    let call_via_table: TypedFunction<(i32, i32, i32), i32> = instance
        .exports
        .get_typed_function(&mut store, "call_callback")?;

    // And then call it with table index 1 and arguments 2 and 7.
    let result = call_via_table.call(&mut store, 1, 2, 7)?;
    // Because it's the default function, we expect it to double each number and
    // then sum it, giving us 18.
    assert_eq!(result, 18);

    // We then get the table from the instance.
    let guest_table = instance.exports.get_table("__indirect_function_table")?;
    // And demonstrate that it has the properties that we set in the Wasm.
    assert_eq!(guest_table.size(&mut store), 3);
    assert_eq!(
        guest_table.ty(&store),
        TableType {
            ty: Type::FuncRef,
            minimum: 3,
            maximum: Some(6)
        }
    );

    // == Setting elements in a table ==

    // We first construct a `Function` over our host_callback.
    let func = Function::new_typed(&mut store, host_callback);

    // And set table index 1 of that table to the host_callback `Function`.
    guest_table.set(&mut store, 1, func.into())?;

    // We then repeat the call from before but this time it will find the host function
    // that we put at table index 1.
    let result = call_via_table.call(&mut store, 1, 2, 7)?;
    // And because our host function simply sums the numbers, we expect 9.
    assert_eq!(result, 9);

    // == Growing a table ==

    // We again construct a `Function` over our host_callback.
    let func = Function::new_typed(&mut store, host_callback);

    // And grow the table by 3 elements, filling in our host_callback in all the
    // new elements of the table.
    let previous_size = guest_table.grow(&mut store, 3, func.into())?;
    assert_eq!(previous_size, 3);

    assert_eq!(guest_table.size(&mut store), 6);
    assert_eq!(
        guest_table.ty(&store),
        TableType {
            ty: Type::FuncRef,
            minimum: 3,
            maximum: Some(6)
        }
    );
    // Now demonstrate that the function we grew the table with is actually in the table.
    for table_index in 3..6 {
        if let Value::FuncRef(Some(f)) = guest_table.get(&mut store, table_index as _).unwrap() {
            let result = f.call(&mut store, &[Value::I32(1), Value::I32(9)])?;
            assert_eq!(result[0], Value::I32(10));
        } else {
            panic!("expected to find funcref in table!");
        }
    }

    // Call function at index 0 to show that it's still the same.
    let result = call_via_table.call(&mut store, 0, 2, 7)?;
    assert_eq!(result, 18);

    // Now overwrite index 0 with our host_callback.
    let func = Function::new_typed(&mut store, host_callback);
    guest_table.set(&mut store, 0, func.into())?;
    // And verify that it does what we expect.
    let result = call_via_table.call(&mut store, 0, 2, 7)?;
    assert_eq!(result, 9);

    // Now demonstrate that the host and guest see the same table and that both
    // get the same result.
    for table_index in 3..6 {
        if let Value::FuncRef(Some(f)) = guest_table.get(&mut store, table_index as _).unwrap() {
            let result = f.call(&mut store, &[Value::I32(1), Value::I32(9)])?;
            assert_eq!(result[0], Value::I32(10));
        } else {
            panic!("expected to find funcref in table!");
        }
        let result = call_via_table.call(&mut store, table_index, 1, 9)?;
        assert_eq!(result, 10);
    }

    Ok(())
}

// This test is currently failing with:
// not implemented: Native function definitions can't be directly called from the host yet
#[test]
fn test_table() -> anyhow::Result<()> {
    main()
}
