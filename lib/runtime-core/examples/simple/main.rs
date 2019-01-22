use wabt::wat2wasm;
use wasmer_clif_backend::CraneliftCompiler;
use wasmer_runtime_core::{self as runtime, error::Result, prelude::*};

static EXAMPLE_WASM: &'static [u8] = include_bytes!("simple.wasm");

fn main() -> Result<()> {
    let wasm_binary = wat2wasm(IMPORT_MODULE.as_bytes()).expect("WAST not valid or malformed");
    let inner_module = runtime::compile(&wasm_binary, &CraneliftCompiler::new())?;

    let import_object = imports! {
        "env" => {
            "print_i32" => print_num<[i32] -> [i32]>,
        },
    };

    let inner_instance = inner_module.instantiate(import_object)?;

    let outer_imports = imports! {
        "env" => inner_instance,
    };

    let outer_module = runtime::compile(EXAMPLE_WASM, &CraneliftCompiler::new())?;
    let mut outer_instance = outer_module.instantiate(outer_imports)?;
    let ret = outer_instance.call("main", &[Value::I32(42)])?;
    println!("ret: {:?}", ret);

    Ok(())
}

extern "C" fn print_num(n: i32, _vmctx: &mut vm::Ctx) -> i32 {
    println!("print_num({})", n);
    n + 1
}

static IMPORT_MODULE: &str = r#"
(module
  (type $t0 (func (param i32) (result i32)))
  (import "env" "print_i32" (func $print_i32 (type $t0)))
  (func $print_num (export "print_num") (type $t0) (param $p0 i32) (result i32)
    get_local $p0
    call $print_i32))
"#;
