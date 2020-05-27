use wasmer::*;

#[test]
fn module_get_name() -> anyhow::Result<()> {
    let store = Store::default();
    let wat = r#"
        (module
        (func (export "run") (nop))
        )
    "#;

    let module = Module::new(&store, wat)?;
    assert_eq!(module.name(), None);

    Ok(())
}

#[test]
fn module_set_name() -> anyhow::Result<()> {
    let store = Store::default();
    let wat = r#"
        (module $from_name_section
        (func (export "run") (nop))
        )
    "#;

    let module = Module::new(&store, wat)?;
    assert_eq!(module.name(), Some("from_name_section"));
    let mut module = Module::new(&store, wat)?;
    module.set_name("override");
    assert_eq!(module.name(), Some("override"));
    Ok(())
}
