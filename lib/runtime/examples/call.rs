use wasmer_runtime::{compile, error, imports, Ctx, Func, Value};

use wabt::wat2wasm;

static WAT: &'static str = r#"
    (module
      (type (;0;) (func (result i32)))
      (import "env" "do_panic" (func $do_panic (type 0)))
      (func $dbz (result i32)
        call $do_panic
        drop
        i32.const 42
        i32.const 0
        i32.div_u
      )
      (export "dbz" (func $dbz))
    )
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

fn foobar(_ctx: &mut Ctx) -> i32 {
    42
}

fn do_panic(_ctx: &mut Ctx) -> Result<i32, String> {
    Err("error".to_string())
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
    let instance = module.instantiate(&imports! {
      "env" => {
          "do_panic" => Func::new(do_panic),
      },
    })?;

    let foo = instance.dyn_func("dbz")?;

    let result = foo.call(&[]);

    println!("result: {:?}", result);

    Ok(())
}
