//! Example demonstrating WebAssembly table growth limits
//!
//! Tests growing a table with a very large delta (0xff_ff_ff_ff) which should fail
//! gracefully by returning -1 according to the WebAssembly specification.
//!
//! You can run the example directly by executing:
//! ```shell
//! cargo run --example table-large-grow --release --features backend
//! ```

use wasmer::{imports, Instance, Module, Store};

fn main() -> anyhow::Result<()> {
    let module_wat = r#"
    (module
      (table $t0 0 externref)
      (table $t1 10 externref)
      (func $init (export "init") (result i32)
	(if (i32.ne (table.size $t1) (i32.const 10))
	  (then (unreachable))
	)
	;; Store the result instead of dropping it
	(table.grow $t0 (ref.null extern) (i32.const 0xff_ff_ff_ff))
      )
    )
    "#;

    let mut store = Store::default();
    let module = Module::new(&store, &module_wat)?;
    let import_object = imports! {};
    let instance = Instance::new(&mut store, &module, &import_object)?;

    let init = instance.exports.get_function("init")?;
    let result = init.call(&mut store, &[])?;

    assert_eq!(
        result[0],
        wasmer::Value::I32(-1),
        "table.grow should return -1"
    );

    Ok(())
}

#[test]
fn test_memory_grow() -> anyhow::Result<()> {
    main()
}
