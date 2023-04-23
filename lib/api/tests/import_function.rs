use macro_wasmer_universal_test::universal_test;
#[cfg(feature = "js")]
use wasm_bindgen_test::*;

use wasmer::*;

#[universal_test]
fn pass_i64_between_host_and_plugin() -> Result<(), String> {
    let mut store = Store::default();

    let wat = r#"(module
        (func $add_one_i64 (import "host" "add_one_i64") (param i64) (result i64))
        (func (export "add_three_i64") (param i64) (result i64)
            (i64.add (call $add_one_i64 (i64.add (local.get 0) (i64.const 1))) (i64.const 1))
        )
    )"#;
    let module = Module::new(&store, wat).map_err(|e| format!("{e:?}"))?;

    let imports = {
        imports! {
            "host" => {
                "add_one_i64" => Function::new_typed(&mut store, |value: i64| value.wrapping_add(1)),
            },
        }
    };

    let instance = Instance::new(&mut store, &module, &imports).map_err(|e| format!("{e:?}"))?;
    let add_three_i64 = instance
        .exports
        .get_typed_function::<i64, i64>(&store, "add_three_i64")
        .map_err(|e| format!("{e:?}"))?;

    let mut numbers = Vec::<i64>::new();
    numbers.extend(-4..=4);
    numbers.extend((i64::MAX - 4)..=i64::MAX);
    numbers.extend((i64::MIN)..=(i64::MIN + 4));

    for number in numbers {
        let wasm_result = add_three_i64
            .call(&mut store, number)
            .map_err(|e| format!("{e:?}"))?;
        let compare_result = number.wrapping_add(3);

        assert_eq!(wasm_result, compare_result)
    }

    Ok(())
}
