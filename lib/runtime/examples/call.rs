use wasmer_runtime::{compile, error, imports, Ctx, Func, Value};

use wabt::wat2wasm;

static WAT: &'static str = r#"
    (module
      (type (;0;) (func (param i32 i32)))
      (type (;1;) (func (param i32 i64)))
      (type (;2;) (func (param i32) (result i32)))
      (type (;3;) (func (param i32) (result i64)))
      (type (;4;) (func (param i64) (result i64)))
      (type (;5;) (func (param f32) (result f32)))
      (type (;6;) (func (param f64) (result f64)))
      (func (;0;) (type 0) (param i32 i32)
        local.get 0
        local.get 1
        i32.store8
        local.get 0
        i32.const 1
        i32.add
        local.get 1
        i32.const 8
        i32.shr_u
        i32.store8)
      (func (;1;) (type 0) (param i32 i32)
        local.get 0
        local.get 1
        call 0
        local.get 0
        i32.const 2
        i32.add
        local.get 1
        i32.const 16
        i32.shr_u
        call 0)
      (func (;2;) (type 1) (param i32 i64)
        local.get 0
        local.get 1
        i32.wrap_i64
        call 1
        local.get 0
        i32.const 4
        i32.add
        local.get 1
        i64.const 32
        i64.shr_u
        i32.wrap_i64
        call 1)
      (func (;3;) (type 2) (param i32) (result i32)
        local.get 0
        i32.load8_u
        local.get 0
        i32.const 1
        i32.add
        i32.load8_u
        i32.const 8
        i32.shl
        i32.or)
      (func (;4;) (type 2) (param i32) (result i32)
        local.get 0
        call 3
        local.get 0
        i32.const 2
        i32.add
        call 3
        i32.const 16
        i32.shl
        i32.or)
      (func (;5;) (type 3) (param i32) (result i64)
        local.get 0
        i64.extend_i32_u
        local.get 0
        call 4
        i64.extend_i32_u
        i64.or)
      (memory (;0;) 1))
"#;

// static WAT2: &'static str = r#"
//     (module
//         (type $t0 (func (param i32)))
//         (type $t1 (func))
//         (func $print_i32 (export "print_i32") (type $t0) (param $lhs i32))
//         (func $print (export "print") (type $t1))
//         (table $table (export "table") 10 20 anyfunc)
//         (memory $memory (export "memory") 1 2)
//         (global $global_i32 (export "global_i32") i32 (i32.const 666)))
// "#;

fn get_wasm() -> Vec<u8> {
    wat2wasm(WAT).unwrap()
}

fn foobar(ctx: &mut Ctx) -> i32 {
    42
}

fn main() -> Result<(), error::Error> {
    let wasm = get_wasm();

    let module = compile(&wasm)?;

    // let import_module = compile(&wat2wasm(WAT2).unwrap())?;
    // let import_instance = import_module.instantiate(&imports! {})?;

    // let imports = imports! {
    //   "spectest" => import_instance,
    // };

    println!("instantiating");
    let instance = module.instantiate(&imports! {})?;

    let foo = instance.dyn_func("four")?;

    let result = foo.call(&[Value::I32(10)]);

    println!("result: {:?}", result);

    Ok(())
}
