use wabt::wat2wasm;
use wasmer_runtime::types::Value;
use wasmer_runtime::{Instance, Imports, module::Module};
use wasmer_clif_backend::CraneliftCompiler;

fn main() {
    let mut instance = create_module_1();
    let result = instance.call("type-first-f32", &[]);
    println!("result: {:?}", result);
}

fn create_module_1() -> Box<Instance> {
    let module_str = "(module
      (type (;0;) (func (result f32)))
      (type (;1;) (func (param f32) (result f32)))
      (func (;0;) (type 1) (param f32) (result f32)
        get_local 0)
      (func (;1;) (type 0) (result f32)
        f32.const 0x1.51eb86p+0 (;=1.32;)
        call 0)
      (export \"func-0\" (func 0))
      (export \"type-first-f32\" (func 1))
    )
    ";
    let wasm_binary = wat2wasm(module_str.as_bytes()).expect("WAST not valid or malformed");
    let module = wasmer_runtime::compile(&wasm_binary[..], &CraneliftCompiler::new()).expect("WASM can't be compiled");
    module.instantiate(&Imports::new()).expect("WASM can't be instantiated")
}