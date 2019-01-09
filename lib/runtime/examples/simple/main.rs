use wasmer_clif_backend::CraneliftCompiler;
use wasmer_runtime::{
    self as runtime,
    types::{FuncSig, Type, Value},
    vm, Import, Imports, FuncRef,
};

static EXAMPLE_WASM: &'static [u8] = include_bytes!("simple.wasm");

fn main() -> Result<(), String> {
    let module = runtime::compile(EXAMPLE_WASM, &CraneliftCompiler::new())?;

    let mut imports = Imports::new();
    imports.add(
        "env".to_string(),
        "print_num".to_string(),
        Import::Func(
            unsafe { FuncRef::new(print_num as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );

    let mut instance = module.instantiate(&imports)?;

    let ret = instance.call("main", &[Value::I32(42)])?;

    println!("ret: {:?}", ret);

    Ok(())
}

extern "C" fn print_num(n: i32, _vmctx: *mut vm::Ctx) -> i32 {
    println!("print_num({})", n);
    n + 1
}
