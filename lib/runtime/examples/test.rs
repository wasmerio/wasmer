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
      (import \"spectest\" \"memory\" (memory (;0;) 0))
      (data (;0;) (i32.const 0) \"\"))
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