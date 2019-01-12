use hashbrown::HashMap;
use std::rc::Rc;
use wabt::wat2wasm;
use wasmer_clif_backend::CraneliftCompiler;
use wasmer_runtime::{
    self as runtime,
    export::{Context, Export, FuncPointer},
    import::Imports,
    types::{FuncSig, Type, Value},
    vm,
};

static EXAMPLE_WASM: &'static [u8] = include_bytes!("simple.wasm");

fn main() -> Result<(), String> {
    let wasm_binary = wat2wasm(IMPORT_MODULE.as_bytes()).expect("WAST not valid or malformed");
    let inner_module = runtime::compile(&wasm_binary, &CraneliftCompiler::new())?;

    let mut env_namespace = HashMap::new();
    env_namespace.insert(
        "print_i32",
        Export::Function {
            func: unsafe { FuncPointer::new(print_num as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![Type::I32],
                returns: vec![Type::I32],
            },
        },
    );
    let mut imports = Imports::new();
    imports.register("env", env_namespace);

    let imports = Rc::new(imports);

    let inner_instance = inner_module.instantiate(imports)?;

    let mut outer_imports = Imports::new();
    outer_imports.register("env", inner_instance);
    let outer_imports = Rc::new(outer_imports);
    let outer_module = runtime::compile(EXAMPLE_WASM, &CraneliftCompiler::new())?;
    let mut outer_instance = outer_module.instantiate(outer_imports)?;
    let ret = outer_instance.call("main", &[Value::I32(42)])?;
    println!("ret: {:?}", ret);

    Ok(())
}

extern "C" fn print_num(n: i32, _vmctx: *mut vm::Ctx) -> i32 {
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
