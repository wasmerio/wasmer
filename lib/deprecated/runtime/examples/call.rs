use wasmer_runtime_deprecated as wasmer_runtime;

use crate::wasmer_runtime::{compile, /*error, error::RuntimeError,*/ imports, Ctx, Func, Value,};

use wabt::wat2wasm;

static WAT: &'static str = r#"

    (module
      (type (;0;) (func (param i32 i32) (result i32)))
      (import "env" "sum" (func $sum (type 0)))
      (func $add_one (type 0)
        local.get 0
        local.get 1
        call $sum
        i32.const 1
        i32.add)
      (export "add_one" (func $add_one))
    )
"#;

fn get_wasm() -> Vec<u8> {
    wat2wasm(WAT).unwrap()
}

fn sum(ctx: &mut Ctx, x: i32, y: i32) -> i32 {
    dbg!(ctx as *const _);
    dbg!(ctx);

    x + y
}

/*
#[derive(Debug)]
struct ExitCode {
    code: i32,
}

fn do_panic(_ctx: &mut Ctx) -> Result<i32, ExitCode> {
    Err(ExitCode { code: 42 })
}
*/

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wasm = get_wasm();
    let module = compile(&wasm)?;

    println!("instantiating");

    let instance = module.instantiate(&imports! {
        "env" => {
            "sum" => Func::new(sum),
        },
    })?;

    let add_one = instance.exports.get_function("add_one")?;
    let result = add_one.call(&[Value::I32(1), Value::I32(2)]);

    println!("result: {:?}", result);

    /*
    if let Err(e) = result {
        if let RuntimeError::User(ue) = e {
            let exit_code = ue.downcast_ref::<ExitCode>().unwrap();
            println!("exit code: {:?}", exit_code);
        } else {
            panic!("Found error that wasn't a user error!: {}", e)
        }
    }
    */

    Ok(())
}
