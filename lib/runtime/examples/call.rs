use wasmer_runtime::{compile, error, imports, Func, Value};

use wabt::wat2wasm;

static WAT: &'static str = r#"
    (module
      (type (;0;) (func (result i32)))
      (func (;0;) (type 0) (result i32)
        block (result i32)  ;; label = @1
          i32.const 1
        end
        return)
      (export "as-return-value" (func 0))
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

    let foo = instance.dyn_func("as-call-value")?;

    let result = foo.call(&[]);

    println!("result: {:?}", result);

    Ok(())
}
