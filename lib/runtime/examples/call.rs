use wasmer_runtime::{compile, error, imports, Func, Value};

use wabt::wat2wasm;

static WAT: &'static str = r#"
    (module
      (type (;0;) (func (result i32)))
      (type (;1;) (func (param i32 i32)))
      (type (;2;) (func (param i32) (result i32)))
      (func (;0;) (type 0) (result i32)
        memory.size
        i32.const 65536
        i32.mul)
      (func (;1;) (type 1) (param i32 i32)
        call 0
        local.get 0
        i32.sub
        local.get 1
        i32.store)
      (func (;2;) (type 2) (param i32) (result i32)
        call 0
        local.get 0
        i32.add
        i32.load)
      (func (;3;) (type 2) (param i32) (result i32)
        local.get 0
        memory.grow)
      (memory (;0;) 1 2)
      (export "store" (func 1))
      (export "load" (func 2))
      (export "memory.grow" (func 3)))
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

    let foo = instance.dyn_func("store")?;

    let result = foo.call(&[Value::I32(0), Value::I32(1)]);

    println!("result: {:?}", result);

    Ok(())
}
