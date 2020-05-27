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
