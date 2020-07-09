use std::{error, fmt};
use wasmer_runtime::{compile, error::RuntimeError, imports, wat2wasm, Ctx, Func};

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

fn get_wasm() -> Vec<u8> {
    wat2wasm(WAT.as_bytes()).unwrap().to_vec()
}

#[derive(Debug)]
struct ExitCode {
    code: i32,
}

impl error::Error for ExitCode {}

impl fmt::Display for ExitCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

fn do_panic(_ctx: &mut Ctx) -> Result<i32, RuntimeError> {
    Err(RuntimeError::new(ExitCode { code: 42 }.to_string()))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wasm = get_wasm();
    let module = compile(&wasm)?;

    println!("instantiating");

    let instance = module.instantiate(&imports! {
        "env" => {
            "do_panic" => Func::new(do_panic),
        },
    })?;

    let foo: Func<(), i32> = instance.exports.get("dbz")?;
    let result = foo.call();

    println!("result: {:?}", result.unwrap_err().message());

    Ok(())
}
