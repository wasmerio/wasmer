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
