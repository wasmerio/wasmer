use anyhow::Result;
use wasmer::*;

#[test]
fn module_get_name() -> Result<()> {
    let store = Store::default();
    let wat = r#"(module)"#;
    let module = Module::new(&store, wat)?;
    assert_eq!(module.name(), None);

    Ok(())
}

#[test]
fn module_set_name() -> Result<()> {
    let store = Store::default();
    let wat = r#"(module $name)"#;
    let mut module = Module::new(&store, wat)?;
    assert_eq!(module.name(), Some("name"));

    module.set_name("new_name");
    assert_eq!(module.name(), Some("new_name"));

    Ok(())
}

#[test]
fn imports() -> Result<()> {
    let store = Store::default();
    let wat = r#"(module
    (import "host" "func" (func))
    (import "host" "memory" (memory 1))
    (import "host" "table" (table 1 anyfunc))
    (import "host" "global" (global i32))
)"#;
    let module = Module::new(&store, wat)?;
    assert_eq!(
        module.imports().collect::<Vec<_>>(),
        vec![
            ImportType::new(
                "host",
                "func",
                ExternType::Function(FunctionType::new(vec![], vec![]))
            ),
            ImportType::new(
                "host",
                "memory",
                ExternType::Memory(MemoryType::new(Pages(1), None, false))
            ),
            ImportType::new(
                "host",
                "table",
                ExternType::Table(TableType::new(Type::FuncRef, 1, None))
            ),
            ImportType::new(
                "host",
                "global",
                ExternType::Global(GlobalType::new(Type::I32, Mutability::Const))
            )
        ]
    );

    // Now we test the iterators
    assert_eq!(
        module.imports().functions().collect::<Vec<_>>(),
        vec![ImportType::new(
            "host",
            "func",
            FunctionType::new(vec![], vec![])
        ),]
    );
    assert_eq!(
        module.imports().memories().collect::<Vec<_>>(),
        vec![ImportType::new(
            "host",
            "memory",
            MemoryType::new(Pages(1), None, false)
        ),]
    );
    assert_eq!(
        module.imports().tables().collect::<Vec<_>>(),
        vec![ImportType::new(
            "host",
            "table",
            TableType::new(Type::FuncRef, 1, None)
        ),]
    );
    assert_eq!(
        module.imports().globals().collect::<Vec<_>>(),
        vec![ImportType::new(
            "host",
            "global",
            GlobalType::new(Type::I32, Mutability::Const)
        ),]
    );
    Ok(())
}

#[test]
fn exports() -> Result<()> {
    let store = Store::default();
    let wat = r#"(module
    (func (export "func") nop)
    (memory (export "memory") 1)
    (table (export "table") 1 funcref)
    (global (export "global") i32 (i32.const 0))
)"#;
    let module = Module::new(&store, wat)?;
    assert_eq!(
        module.exports().collect::<Vec<_>>(),
        vec![
            ExportType::new(
                "func",
                ExternType::Function(FunctionType::new(vec![], vec![]))
            ),
            ExportType::new(
                "memory",
                ExternType::Memory(MemoryType::new(Pages(1), None, false))
            ),
            ExportType::new(
                "table",
                ExternType::Table(TableType::new(Type::FuncRef, 1, None))
            ),
            ExportType::new(
                "global",
                ExternType::Global(GlobalType::new(Type::I32, Mutability::Const))
            )
        ]
    );

    // Now we test the iterators
    assert_eq!(
        module.exports().functions().collect::<Vec<_>>(),
        vec![ExportType::new("func", FunctionType::new(vec![], vec![])),]
    );
    assert_eq!(
        module.exports().memories().collect::<Vec<_>>(),
        vec![ExportType::new(
            "memory",
            MemoryType::new(Pages(1), None, false)
        ),]
    );
    assert_eq!(
        module.exports().tables().collect::<Vec<_>>(),
        vec![ExportType::new(
            "table",
            TableType::new(Type::FuncRef, 1, None)
        ),]
    );
    assert_eq!(
        module.exports().globals().collect::<Vec<_>>(),
        vec![ExportType::new(
            "global",
            GlobalType::new(Type::I32, Mutability::Const)
        ),]
    );
    Ok(())
}

#[test]
fn native_function_works_for_wasm() -> Result<()> {
    let store = Store::default();
    let wat = r#"(module
    (func $multiply (import "env" "multiply") (param i32 i32) (result i32))
    (func (export "add") (param i32 i32) (result i32)
       (i32.add (local.get 0)
                (local.get 1)))
    (func (export "double_then_add") (param i32 i32) (result i32)
       (i32.add (call $multiply (local.get 0) (i32.const 2))
                (call $multiply (local.get 1) (i32.const 2))))
)"#;
    let module = Module::new(&store, wat)?;

    let import_object = imports! {
        "env" => {
            "multiply" => Function::new(&store, |a: i32, b: i32| a * b),
        },
    };

    let instance = Instance::new(&module, &import_object).unwrap();

    // TODO:
    //let f: NativeFunc<(i32, i32), i32> = instance.exports.get("add").unwrap();
    let dyn_f: &Function = instance.exports.get("add").unwrap();
    let dyn_result = dyn_f.call(&[Val::I32(4), Val::I32(6)]).unwrap();
    assert_eq!(dyn_result[0], Val::I32(10));

    let f: NativeFunc<(i32, i32), i32> = dyn_f.clone().native().unwrap();

    let result = f.call(4, 6).unwrap();
    assert_eq!(result, 10);

    let dyn_f: &Function = instance.exports.get("double_then_add").unwrap();
    let dyn_result = dyn_f.call(&[Val::I32(4), Val::I32(6)]).unwrap();
    assert_eq!(dyn_result[0], Val::I32(20));

    let f: NativeFunc<(i32, i32), i32> = dyn_f.clone().native().unwrap();

    let result = f.call(4, 6).unwrap();
    assert_eq!(result, 20);

    Ok(())
}
