use crate::utils::get_store_with_middlewares;
use anyhow::Result;
use wasmer_middlewares::Metering;

use std::sync::Arc;
use wasmer::wasmparser::{Operator, Result as WpResult};
use wasmer::*;

fn cost_always_one(_: &Operator) -> u64 {
    1
}

#[test]
fn metering_middleware() -> Result<()> {
    let store = get_store_with_middlewares(std::iter::once(Arc::new(Metering::new(
        4,
        cost_always_one,
    )) as Arc<dyn ModuleMiddleware>));
    let wat = r#"(module
        (func (export "add") (param i32 i32) (result i32)
           (i32.add (local.get 0)
                    (local.get 1)))
)"#;
    let module = Module::new(&store, wat).unwrap();

    let import_object = imports! {};

    let instance = Instance::new(&module, &import_object)?;

    let f: NativeFunc<(i32, i32), i32> = instance.exports.get_native_function("add")?;
    let result = f.call(4, 6)?;
    assert_eq!(result, 10);
    Ok(())
}
