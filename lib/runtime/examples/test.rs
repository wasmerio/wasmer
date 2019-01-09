use wabt::wat2wasm;
use wasmer_runtime::types::Value;
use wasmer_runtime::{Instance, Imports, module::Module};
use wasmer_clif_backend::CraneliftCompiler;

fn main() {
    let mut instance = create_module_1();
    let result = instance.call("func-0", &[]);
    println!("result: {:?}", result);
}

fn create_module_1() -> Box<Instance> {
    let module_str = "(module
      (type (;0;) (func (result i32)))
      (func (;0;) (type 0) (result i32)
        i32.const 306)
      (func (;1;) (type 0) (result i32)
        call 0)
      (export \"type-i32\" (func 1))
      (export \"func-0\" (func 0))
    )
    ";
    let wasm_binary = wat2wasm(module_str.as_bytes()).expect("WAST not valid or malformed");
    let module = wasmer_runtime::compile(&wasm_binary[..], &CraneliftCompiler::new()).expect("WASM can't be compiled");
    module.instantiate(&Imports::new()).expect("WASM can't be instantiated")
}