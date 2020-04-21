use wabt::wat2wasm;
use wasmer_runtime::{
    compile, error, func, imports,
    types::{ElementType, MemoryType, TableType, Value},
    units::Pages,
    Ctx, Global, Memory, Table,
};

static EXAMPLE_WASM: &'static [u8] = include_bytes!("simple.wasm");

fn main() -> error::Result<()> {
    let wasm_binary = wat2wasm(IMPORT_MODULE.as_bytes()).expect("WAST not valid or malformed");

    let inner_module = compile(&wasm_binary)?;

    let memory_desc = MemoryType::new(Pages(1), Some(Pages(1)), false).unwrap();
    let memory = Memory::new(memory_desc).unwrap();

    let global = Global::new(Value::I32(42));

    let table = Table::new(TableType {
        element: ElementType::Anyfunc,
        minimum: 10,
        maximum: None,
    })
    .unwrap();

    memory.view()[0].set(42);

    let import_object = imports! {
        "env" => {
            "print_i32" => func!(print_num),
            "memory" => memory,
            "global" => global,
            "table" => table,
        },
    };

    let inner_instance = inner_module.instantiate(&import_object)?;

    let outer_imports = imports! {
        "env" => inner_instance,
    };

    let outer_module = compile(EXAMPLE_WASM)?;
    let outer_instance = outer_module.instantiate(&outer_imports)?;
    let ret = outer_instance.call("main", &[Value::I32(42)])?;
    println!("ret: {:?}", ret);

    Ok(())
}

fn print_num(ctx: &mut Ctx, n: i32) -> Result<i32, ()> {
    println!("print_num({})", n);

    let memory: &Memory = ctx.memory(0);

    let a: i32 = memory.view()[0].get();

    Ok(a + n + 1)
}

static IMPORT_MODULE: &str = r#"
(module
  (type $t0 (func (param i32) (result i32)))
  (import "env" "memory" (memory 1 1))
  (import "env" "table" (table 10 anyfunc))
  (import "env" "global" (global i32))
  (import "env" "print_i32" (func $print_i32 (type $t0)))
  (func $identity (type $t0) (param $p0 i32) (result i32)
    get_local $p0)
  (func $print_num (export "print_num") (type $t0) (param $p0 i32) (result i32)
    get_global 0
    call $identity
    call $print_i32))
"#;
