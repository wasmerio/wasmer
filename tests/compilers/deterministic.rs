use anyhow::Result;
use wasmer::{Module, wat2wasm};

fn compile_and_compare(config: crate::Config, wasm: &[u8]) -> Result<()> {
    let store = config.store();

    // compile for first time
    let module = Module::new(&store, wasm)?;
    let first = module.serialize()?;

    // compile for second time
    let module = Module::new(&store, wasm)?;
    let second = module.serialize()?;

    assert!(first == second);

    Ok(())
}

#[compiler_test(deterministic)]
fn deterministic_empty(config: crate::Config) -> Result<()> {
    let wasm_bytes = wat2wasm(
        br#"
    (module)
    "#,
    )?;

    compile_and_compare(config, &wasm_bytes)
}

#[compiler_test(deterministic)]
fn deterministic_table(config: crate::Config) -> Result<()> {
    let wasm_bytes = wat2wasm(
        br#"
(module
  (table 2 funcref)
  (func $f1)
  (func $f2)
  (elem (i32.const 0) $f1 $f2))
"#,
    )?;

    compile_and_compare(config, &wasm_bytes)
}
