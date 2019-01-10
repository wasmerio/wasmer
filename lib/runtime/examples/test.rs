use wabt::wat2wasm;
use wasmer_runtime::{Instance, Imports, Import, FuncRef, table::TableBacking, types::{Value, Type, Table, FuncSig, ElementType}, module::Module};
use std::sync::Arc;
use wasmer_clif_backend::CraneliftCompiler;

fn main() {
    let mut instance = create_module_1();
    let result = instance.call("signature-implicit-reused", &[]);
    println!("result: {:?}", result);
}

fn create_module_1() -> Box<Instance> {
    let module_str = "(module
      (type (;0;) (func))
      (type (;1;) (func))
      (type (;2;) (func (param i64 i64 f64 i64 f64 i64 f32 i32)))
      (type (;3;) (func (param f64 i64 f64 i64 f64 i64 f32 i32)))
      (func (;0;) (type 0))
      (func (;1;) (type 3) (param f64 i64 f64 i64 f64 i64 f32 i32))
      (func (;2;) (type 0))
      (func (;3;) (type 3) (param f64 i64 f64 i64 f64 i64 f32 i32))
      (func (;4;) (type 3) (param f64 i64 f64 i64 f64 i64 f32 i32))
      (func (;5;) (type 2) (param i64 i64 f64 i64 f64 i64 f32 i32))
      (func (;6;) (type 2) (param i64 i64 f64 i64 f64 i64 f32 i32))
      (func (;7;) (type 0)
        f64.const 0x0p+0 (;=0;)
        i64.const 0
        f64.const 0x0p+0 (;=0;)
        i64.const 0
        f64.const 0x0p+0 (;=0;)
        i64.const 0
        f32.const 0x0p+0 (;=0;)
        i32.const 0
        i32.const 0
        call_indirect (type 3)
        f64.const 0x0p+0 (;=0;)
        i64.const 0
        f64.const 0x0p+0 (;=0;)
        i64.const 0
        f64.const 0x0p+0 (;=0;)
        i64.const 0
        f32.const 0x0p+0 (;=0;)
        i32.const 0
        i32.const 2
        call_indirect (type 3)
        f64.const 0x0p+0 (;=0;)
        i64.const 0
        f64.const 0x0p+0 (;=0;)
        i64.const 0
        f64.const 0x0p+0 (;=0;)
        i64.const 0
        f32.const 0x0p+0 (;=0;)
        i32.const 0
        i32.const 3
        call_indirect (type 3))
      (table (;0;) 7 7 anyfunc)
      (export \"signature-implicit-reused\" (func 7))
      (elem (;0;) (i32.const 0) 4 2 1 4 0 5 6))
    ";
    let wasm_binary = wat2wasm(module_str.as_bytes()).expect("WAST not valid or malformed");
    let module = wasmer_runtime::compile(&wasm_binary[..], &CraneliftCompiler::new()).expect("WASM can't be compiled");
    module.instantiate(&spectest_importobject()).expect("WASM can't be instantiated")
}

extern "C" fn print_i32(num: i32) {
    println!("{}", num);
}

extern "C" fn print() {}

static GLOBAL_I32: i32 = 666;

pub fn spectest_importobject() -> Imports {
    let mut import_object = Imports::new();

    import_object.add(
        "spectest",
        "print_i32",
        Import::Func(
            unsafe { FuncRef::new(print_i32 as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![],
            },
        ),
    );

    import_object.add(
        "spectest",
        "print",
        Import::Func(
            unsafe { FuncRef::new(print as _) },
            FuncSig {
                params: vec![],
                returns: vec![],
            },
        ),
    );

    import_object.add(
        "spectest".to_string(),
        "global_i32".to_string(),
        Import::Global(Value::I64(GLOBAL_I32 as _)),
    );

    let table = Table {
        ty: ElementType::Anyfunc,
        min: 0,
        max: Some(30),
    };
    import_object.add(
        "spectest".to_string(),
        "table".to_string(),
        Import::Table(Arc::new(TableBacking::new(&table)), table),
    );

    return import_object;
}