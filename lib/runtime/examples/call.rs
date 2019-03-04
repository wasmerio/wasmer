use wasmer_runtime::{compile, error, imports, Ctx, Func, Value};

use wabt::wat2wasm;

static WAT: &'static str = r#"
    (module
      (type (;0;) (func))
      (type (;1;) (func))
      (type (;2;) (func))
      (type (;3;) (func (result i32)))
      (type (;4;) (func (result i32)))
      (type (;5;) (func (param i32) (result i32)))
      (type (;6;) (func (param i32)))
      (import "spectest" "print_i32" (func (;0;) (type 6)))
      (func (;1;) (type 0))
      (func (;2;) (type 1))
      (func (;3;) (type 4) (result i32)
        i32.const 13)
      (func (;4;) (type 5) (param i32) (result i32)
        local.get 0
        i32.const 1
        i32.add)
      (func (;5;) (type 5) (param i32) (result i32)
        local.get 0
        i32.const 2
        i32.sub)
      (func (;6;) (type 6) (param i32)
        local.get 0
        call 0)
      (export "one" (func 3))
      (export "two" (func 4))
      (export "three" (func 5))
      (export "four" (func 6)))
"#;

static WAT2: &'static str = r#"
    (module
        (type $t0 (func (param i32)))
        (type $t1 (func))
        (func $print_i32 (export "print_i32") (type $t0) (param $lhs i32))
        (func $print (export "print") (type $t1))
        (table $table (export "table") 10 20 anyfunc)
        (memory $memory (export "memory") 1 2)
        (global $global_i32 (export "global_i32") i32 (i32.const 666)))
"#;

fn get_wasm() -> Vec<u8> {
    wat2wasm(WAT).unwrap()
}

fn foobar(ctx: &mut Ctx) -> i32 {
    42
}

fn main() -> Result<(), error::Error> {
    let wasm = get_wasm();

    let module = compile(&wasm)?;

    let import_module = compile(&wat2wasm(WAT2).unwrap())?;
    let import_instance = import_module.instantiate(&imports! {})?;

    let imports = imports! {
      "spectest" => import_instance,
    };

    println!("instantiating");
    let instance = module.instantiate(&imports)?;

    let foo = instance.dyn_func("four")?;

    let result = foo.call(&[Value::I32(10)]);

    println!("result: {:?}", result);

    Ok(())
}
