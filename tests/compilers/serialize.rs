use anyhow::Result;
use wasmer::{sys::engine::NativeEngineExt, *};

#[test]
fn sanity_test_artifact_deserialize() {
    let engine = Engine::headless();
    let result = unsafe { Module::deserialize(&engine, &[]) };
    assert!(result.is_err());
}

#[compiler_test(serialize)]
fn test_serialize(config: crate::Config) -> Result<()> {
    let mut store = config.store();
    let wat = r#"
        (module
        (func $hello (import "" "hello"))
        (func (export "run") (call $hello))
        )
    "#;

    let module = Module::new(&store, wat)?;
    let serialized_bytes = module.serialize()?;
    assert!(!serialized_bytes.is_empty());
    Ok(())
}

#[compiler_test(serialize)]
fn test_deserialize(config: crate::Config) -> Result<()> {
    let mut store = config.store();
    let wat = r#"
        (module $name
            (import "host" "sum_part" (func (param i32 i64 i32 f32 f64) (result i64)))
            (func (export "test_call") (result i64)
                i32.const 100
                i64.const 200
                i32.const 300
                f32.const 400
                f64.const 500
                call 0
            )
        )
    "#;

    let module = Module::new(&store, wat)?;
    let serialized_bytes = module.serialize()?;

    let headless_store = config.headless_store();
    let deserialized_module = unsafe { Module::deserialize(&headless_store, serialized_bytes)? };
    assert_eq!(deserialized_module.name(), Some("name"));
    assert_eq!(
        deserialized_module.exports().collect::<Vec<_>>(),
        module.exports().collect::<Vec<_>>()
    );
    assert_eq!(
        deserialized_module.imports().collect::<Vec<_>>(),
        module.imports().collect::<Vec<_>>()
    );

    let func_type = FunctionType::new(
        vec![Type::I32, Type::I64, Type::I32, Type::F32, Type::F64],
        vec![Type::I64],
    );
    let f0 = Function::new(&mut store, &func_type, |params| {
        let param_0: i64 = params[0].unwrap_i32() as i64;
        let param_1: i64 = params[1].unwrap_i64();
        let param_2: i64 = params[2].unwrap_i32() as i64;
        let param_3: i64 = params[3].unwrap_f32() as i64;
        let param_4: i64 = params[4].unwrap_f64() as i64;
        Ok(vec![Value::I64(
            param_0 + param_1 + param_2 + param_3 + param_4,
        )])
    });
    let instance = Instance::new(
        &mut store,
        &module,
        &imports! {
            "host" => {
                "sum_part" => f0
            }
        },
    )?;

    let test_call = instance.exports.get_function("test_call")?;
    let result = test_call.call(&mut store, &[])?;
    assert_eq!(result.to_vec(), vec![Value::I64(1500)]);
    Ok(())
}
