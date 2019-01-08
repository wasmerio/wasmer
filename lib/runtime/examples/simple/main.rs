use wasmer_runtime as runtime;
use wasmer_clif_backend::CraneliftCompiler;

static EXAMPLE_WASM: &'static [u8] = include_bytes!("simple.wasm");

fn main() {
    let compiler = CraneliftCompiler::new();
    let module = runtime::compile(EXAMPLE_WASM, &compiler).unwrap();
    let imports = runtime::Imports::new();
    let mut instance = module.instantiate(&imports).unwrap();
    let ret = instance.call("main", &[runtime::types::Value::I32(42)]);
    println!("ret: {:?}", ret);
}