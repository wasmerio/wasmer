use anyhow::Result;
use wasmer::*;

#[test]
fn store_dropped_twice() -> Result<()> {
    let wat = r#"(module)"#;

    let store = Store::default();
    let _module = Module::new(&store, wat)?;

    let store = Store::default();
    let _module = Module::new(&store, wat)?;

    Ok(())
}
