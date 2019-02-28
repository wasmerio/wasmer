use wasmer_runtime::{compile, error, imports, Func};

use wabt::wat2wasm;

static WAT: &'static str = r#"
    (module
        (type $t0 (func (param i32) (result i32)))
        (type $t1 (func (result i32)))
        (memory 1)
        (global $g0 (mut i32) (i32.const 0))
        (export "foo" (func $foo))
        (func $foo (type $t0) (param i32) (result i32)
            get_local 0
        )
    )
"#;

fn get_wasm() -> Vec<u8> {
    wat2wasm(WAT).unwrap()
}

fn main() -> Result<(), error::Error> {
    let wasm = get_wasm();

    let module = compile(&wasm)?;

    let imports = imports! {};

    let instance = module.instantiate(&imports)?;

    let foo: Func<i32, i32> = instance.func("foo")?;

    let result = foo.call(42);

    println!("result: {:?}", result);

    Ok(())
}
