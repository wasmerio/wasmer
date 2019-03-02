use wasmer_runtime::{compile, error, imports, Func, Value};

use wabt::wat2wasm;

static WAT: &'static str = r#"
    (module
      (type (;0;) (func (param i32) (result i32)))
      (func (;0;) (type 0) (param i32) (result i32)
        unreachable)
      (export "select_trap_l" (func 0))
    )
"#;

fn get_wasm() -> Vec<u8> {
    wat2wasm(WAT).unwrap()
}

fn main() -> Result<(), error::Error> {
    let wasm = get_wasm();

    let module = compile(&wasm)?;

    let imports = imports! {};

    println!("instantiating");
    let instance = module.instantiate(&imports)?;

    let foo = instance.dyn_func("select_trap_l")?;

    let result = foo.call(&[Value::I32(0)]);

    println!("result: {:?}", result);

    Ok(())
}
