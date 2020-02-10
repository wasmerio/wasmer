use wabt::wat2wasm;
use wasmer_runtime::{compile, ImportObject, Instance};

fn main() {
    let instance = create_module_1();
    let result = instance.call("call-overwritten-element", &[]);
    println!("result: {:?}", result);
}

fn create_module_1() -> Instance {
    let module_str = r#"(module
      (type (;0;) (func (result i32)))
      (import "spectest" "table" (table (;0;) 10 anyfunc))
      (func (;0;) (type 0) (result i32)
        i32.const 65)
      (func (;1;) (type 0) (result i32)
        i32.const 66)
      (func (;2;) (type 0) (result i32)
        i32.const 9
        call_indirect (type 0))
      (export "call-overwritten-element" (func 2))
      (elem (;0;) (i32.const 9) 0)
      (elem (;1;) (i32.const 9) 1))
    "#;
    let wasm_binary = wat2wasm(module_str.as_bytes()).expect("WAST not valid or malformed");
    let module = compile(&wasm_binary[..]).expect("WASM can't be compiled");
    module
        .instantiate(&generate_imports())
        .expect("WASM can't be instantiated")
}

static IMPORT_MODULE: &str = r#"
(module
  (type $t0 (func (param i32)))
  (type $t1 (func))
  (func $print_i32 (export "print_i32") (type $t0) (param $lhs i32))
  (func $print (export "print") (type $t1))
  (table $table (export "table") 10 20 anyfunc)
  (memory $memory (export "memory") 1 2)
  (global $global_i32 (export "global_i32") i32 (i32.const 666)))
"#;

pub fn generate_imports() -> ImportObject {
    let wasm_binary = wat2wasm(IMPORT_MODULE.as_bytes()).expect("WAST not valid or malformed");
    let module = compile(&wasm_binary[..]).expect("WASM can't be compiled");
    let instance = module
        .instantiate(&ImportObject::new())
        .expect("WASM can't be instantiated");
    let mut imports = ImportObject::new();
    imports.register("spectest", instance);
    imports
}
