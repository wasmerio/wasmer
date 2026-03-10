#[test]
fn allow_missing() {
    use wabt::wat2wasm;
    use wasmer_runtime::{imports, instantiate};

    static WAT: &'static str = r#"
        (module
        (type (;0;) (func))
        (type (;1;) (func (result i32)))
        (import "env" "ret_err" (func $ret_err (type 0)))
        (func $get_num (type 1)
            i32.const 42
        )
        (export "get_num" (func $get_num))
        )
    "#;

    let wasm = wat2wasm(WAT).unwrap();

    let mut import_object = imports! {};
    import_object.allow_missing_functions = true;

    assert!(instantiate(&wasm, &import_object).is_ok());
}
