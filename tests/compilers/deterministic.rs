use anyhow::Result;
use wasmer::{wat2wasm, Module, Store};

fn compile_and_compare(wasm: &[u8]) -> Result<()> {
    let store = Store::default();

    // compile for first time
    let module = Module::new(&store, wasm)?;
    let first = module.serialize()?;

    // compile for second time
    let module = Module::new(&store, wasm)?;
    let second = module.serialize()?;

    assert!(first == second);

    Ok(())
}

#[test]
fn deterministic_empty() -> Result<()> {
    let wasm_bytes = wat2wasm(
        br#"
    (module)
    "#,
    )?;

    compile_and_compare(&wasm_bytes)
}

#[test]
fn deterministic_table() -> Result<()> {
    let wasm_bytes = wat2wasm(
        br#"
(module
  (table 2 funcref)
  (func $f1)
  (func $f2)
  (elem (i32.const 0) $f1 $f2))
"#,
    )?;

    compile_and_compare(&wasm_bytes)
}
