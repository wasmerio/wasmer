use macro_wasmer_universal_test::universal_test;
#[cfg(feature = "js")]
use wasm_bindgen_test::*;

use anyhow::Result;
use wasmer::*;

#[universal_test]
fn calling_function_exports() -> Result<()> {
    let mut store = Store::default();
    let wat = r#"(module
    (func (export "add") (param $lhs i32) (param $rhs i32) (result i32)
        local.get $lhs
        local.get $rhs
        i32.add)
)"#;
    let module = Module::new(&store, wat)?;
    let imports = imports! {
        // "host" => {
        //     "host_func1" => Function::new_typed(&mut store, |p: u64| {
        //         println!("host_func1: Found number {}", p);
        //         // assert_eq!(p, u64::max_value());
        //     }),
        // }
    };
    let instance = Instance::new(&mut store, &module, &imports)?;

    let add: TypedFunction<(i32, i32), i32> =
        instance.exports.get_typed_function(&mut store, "add")?;

    let result = add.call(&mut store, 10, 20)?;
    assert_eq!(result, 30);

    Ok(())
}

#[universal_test]
fn back_and_forth_with_imports() -> Result<()> {
    let mut store = Store::default();
    // We can use the WAT syntax as well!
    let module = Module::new(
        &store,
        br#"(module
            (func $sum (import "env" "sum") (param i32 i32) (result i32))
            (func (export "add_one") (param i32) (result i32)
                (call $sum (local.get 0) (i32.const 1))
            )
        )"#,
    )?;

    fn sum(a: i32, b: i32) -> i32 {
        println!("Summing: {}+{}", a, b);
        a + b
    }

    let import_object = imports! {
        "env" => {
            "sum" => Function::new_typed(&mut store, sum),
        }
    };
    let instance = Instance::new(&mut store, &module, &import_object)?;

    let add_one: TypedFunction<i32, i32> =
        instance.exports.get_typed_function(&mut store, "add_one")?;
    add_one.call(&mut store, 1)?;

    Ok(())
}
