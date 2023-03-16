use macro_wasmer_universal_test::universal_test;
#[cfg(feature = "js")]
use wasm_bindgen_test::*;

use wasmer::*;

#[universal_test]
fn calling_function_exports() -> Result<(), String> {
    let mut store = Store::default();
    let wat = r#"(module
    (func (export "add") (param $lhs i32) (param $rhs i32) (result i32)
        local.get $lhs
        local.get $rhs
        i32.add)
)"#;
    let module = Module::new(&store, wat).map_err(|e| format!("{e:?}"))?;
    let imports = imports! {
        // "host" => {
        //     "host_func1" => Function::new_typed(&mut store, |p: u64| {
        //         println!("host_func1: Found number {}", p);
        //         // assert_eq!(p, u64::max_value());
        //     }),
        // }
    };
    let instance = Instance::new(&mut store, &module, &imports).map_err(|e| format!("{e:?}"))?;

    let add: TypedFunction<(i32, i32), i32> = instance
        .exports
        .get_typed_function(&mut store, "add")
        .map_err(|e| format!("{e:?}"))?;

    let result = add.call(&mut store, 10, 20).map_err(|e| format!("{e:?}"))?;
    assert_eq!(result, 30);

    Ok(())
}
