use std::rc::Rc;
use wabt::wat2wasm;
use wasmer_clif_backend::CraneliftCompiler;
use wasmer_runtime::{
    import::Imports,
    Instance,
};

fn main() {
    let mut instance = create_module_1();
    let result = instance.call("type-i64", &[]);
    println!("result: {:?}", result);
}

fn generate_imports() -> Rc<Imports> {
    // let wasm_binary = wat2wasm(IMPORT_MODULE.as_bytes()).expect("WAST not valid or malformed");
    // let module = wasmer_runtime::compile(&wasm_binary[..], &CraneliftCompiler::new()).expect("WASM can't be compiled");
    // let instance = module.instantiate(Rc::new(Imports::new())).expect("WASM can't be instantiated");
    let imports = Imports::new();
    // imports.register("spectest", instance);
    Rc::new(imports)
}

fn create_module_1() -> Instance {
    let module_str = "(module
      (type (;0;) (func (result i64)))
      (func (;0;) (type 0) (result i64)
        i64.const 356)
      (func (;1;) (type 0) (result i64)
        i32.const 0
        call_indirect (type 0))
      (table (;0;) 2 anyfunc)
      (export \"type-i64\" (func 1))
      (elem (;0;) (i32.const 0) 0 1))
    ";
    let wasm_binary = wat2wasm(module_str.as_bytes()).expect("WAST not valid or malformed");
    let module = wasmer_runtime::compile(&wasm_binary[..], &CraneliftCompiler::new())
        .expect("WASM can't be compiled");
    module
        .instantiate(generate_imports())
        .expect("WASM can't be instantiated")
}
