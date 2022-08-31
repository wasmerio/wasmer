use anyhow::Result;
#[cfg(feature = "js")]
use wasm_bindgen_test::*;

use wasmer::*;

#[cfg(feature = "js")]
#[cfg_attr(feature = "js", wasm_bindgen_test)]
fn module_get_name_js() {
    module_get_name().unwrap();    
}

#[cfg_attr(feature = "sys", test)]
fn module_get_name() -> Result<()> {
    let store = Store::default();
    let wat = r#"(module)"#;
    let module = Module::new(&store, wat)?;
    assert_eq!(module.name(), None);

    Ok(())
}

#[cfg(feature = "js")]
#[cfg_attr(feature = "js", wasm_bindgen_test)]
fn module_set_name_js() {
    module_set_name().unwrap();    
}

#[cfg_attr(feature = "sys", test)]
fn module_set_name() -> Result<()> {
    let store = Store::default();
    let wat = r#"(module $name)"#;
    let mut module = Module::new(&store, wat)?;
    assert_eq!(module.name(), Some("name"));

    module.set_name("new_name");
    assert_eq!(module.name(), Some("new_name"));

    Ok(())
}

#[cfg(feature = "js")]
#[cfg_attr(feature = "js", wasm_bindgen_test)]
fn imports_js() {
    imports().unwrap();    
}

#[cfg_attr(feature = "sys", test)]
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

#[cfg(feature = "js")]
#[cfg_attr(feature = "js", wasm_bindgen_test)]
fn exports_js() {
    exports().unwrap();    
}

#[cfg_attr(feature = "sys", test)]
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

#[cfg(feature = "js")]
#[cfg_attr(feature = "js", wasm_bindgen_test)]
fn calling_host_functions_with_negative_values_works_js() {
    calling_host_functions_with_negative_values_works().unwrap();    
}

#[cfg_attr(feature = "sys", test)]
fn calling_host_functions_with_negative_values_works() -> Result<()> {
    let mut store = Store::default();
    let wat = r#"(module
(import "host" "host_func1" (func (param i64)))
(import "host" "host_func2" (func (param i32)))
(import "host" "host_func3" (func (param i64)))
(import "host" "host_func4" (func (param i32)))
(import "host" "host_func5" (func (param i32)))
(import "host" "host_func6" (func (param i32)))
(import "host" "host_func7" (func (param i32)))
(import "host" "host_func8" (func (param i32)))

(func (export "call_host_func1")
      (call 0 (i64.const -1)))
(func (export "call_host_func2")
      (call 1 (i32.const -1)))
(func (export "call_host_func3")
      (call 2 (i64.const -1)))
(func (export "call_host_func4")
      (call 3 (i32.const -1)))
(func (export "call_host_func5")
      (call 4 (i32.const -1)))
(func (export "call_host_func6")
      (call 5 (i32.const -1)))
(func (export "call_host_func7")
      (call 6 (i32.const -1)))
(func (export "call_host_func8")
      (call 7 (i32.const -1)))
)"#;
    let module = Module::new(&store, wat)?;
    let imports = imports! {
        "host" => {
            "host_func1" => Function::new_typed(&mut store, |p: u64| {
                println!("host_func1: Found number {}", p);
                assert_eq!(p, u64::max_value());
            }),
            "host_func2" => Function::new_typed(&mut store, |p: u32| {
                println!("host_func2: Found number {}", p);
                assert_eq!(p, u32::max_value());
            }),
            "host_func3" => Function::new_typed(&mut store, |p: i64| {
                println!("host_func3: Found number {}", p);
                assert_eq!(p, -1);
            }),
            "host_func4" => Function::new_typed(&mut store, |p: i32| {
                println!("host_func4: Found number {}", p);
                assert_eq!(p, -1);
            }),
            "host_func5" => Function::new_typed(&mut store, |p: i16| {
                println!("host_func5: Found number {}", p);
                assert_eq!(p, -1);
            }),
            "host_func6" => Function::new_typed(&mut store, |p: u16| {
                println!("host_func6: Found number {}", p);
                assert_eq!(p, u16::max_value());
            }),
            "host_func7" => Function::new_typed(&mut store, |p: i8| {
                println!("host_func7: Found number {}", p);
                assert_eq!(p, -1);
            }),
            "host_func8" => Function::new_typed(&mut store, |p: u8| {
                println!("host_func8: Found number {}", p);
                assert_eq!(p, u8::max_value());
            }),
        }
    };
    let instance = Instance::new(&mut store, &module, &imports)?;

    let f1: TypedFunction<(), ()> = instance
        .exports
        .get_typed_function(&mut store, "call_host_func1")?;
    let f2: TypedFunction<(), ()> = instance
        .exports
        .get_typed_function(&mut store, "call_host_func2")?;
    let f3: TypedFunction<(), ()> = instance
        .exports
        .get_typed_function(&mut store, "call_host_func3")?;
    let f4: TypedFunction<(), ()> = instance
        .exports
        .get_typed_function(&mut store, "call_host_func4")?;
    let f5: TypedFunction<(), ()> = instance
        .exports
        .get_typed_function(&mut store, "call_host_func5")?;
    let f6: TypedFunction<(), ()> = instance
        .exports
        .get_typed_function(&mut store, "call_host_func6")?;
    let f7: TypedFunction<(), ()> = instance
        .exports
        .get_typed_function(&mut store, "call_host_func7")?;
    let f8: TypedFunction<(), ()> = instance
        .exports
        .get_typed_function(&mut store, "call_host_func8")?;

    f1.call(&mut store)?;
    f2.call(&mut store)?;
    f3.call(&mut store)?;
    f4.call(&mut store)?;
    f5.call(&mut store)?;
    f6.call(&mut store)?;
    f7.call(&mut store)?;
    f8.call(&mut store)?;

    Ok(())
}