use std::sync::OnceLock;

use anyhow::Result;
use futures::future;
use wasmer::{
    Function, FunctionEnv, FunctionEnvMut, FunctionType, Instance, Module, Store, Type, Value,
    imports,
};

#[derive(Default)]
struct DeltaState {
    deltas: Vec<f64>,
    index: usize,
}

impl DeltaState {
    fn next(&mut self) -> f64 {
        let value = self.deltas.get(self.index).copied().unwrap_or(0.0);
        self.index += 1;
        value
    }
}

fn jspi_module() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    const JSPI_WAT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/examples/jspi.wat");
    BYTES.get_or_init(|| wat::parse_file(JSPI_WAT).expect("valid example module"))
}

#[test]
fn async_state_updates_follow_jspi_example() -> Result<()> {
    let wasm = jspi_module();
    let mut store = Store::default();
    let module = Module::new(&store, wasm)?;

    let init_state = Function::new_async(
        &mut store,
        FunctionType::new(vec![], vec![Type::F64]),
        |_values| async move {
            // Note: future::ready doesn't actually suspend. It's important
            // to note that, while we're in an async context here, it's
            // impossible to suspend during module instantiation, which is
            // where this import is called.
            // To see this in action, uncomment the following line:
            // tokio::task::yield_now().await;
            future::ready(()).await;
            Ok(vec![Value::F64(1.0)])
        },
    );

    let delta_env = FunctionEnv::new(
        &mut store,
        DeltaState {
            deltas: vec![0.5, -1.0, 2.5],
            index: 0,
        },
    );
    let compute_delta = Function::new_with_env_async(
        &mut store,
        &delta_env,
        FunctionType::new(vec![], vec![Type::F64]),
        |mut env: FunctionEnvMut<DeltaState>, _values| {
            let delta = env.data_mut().next();
            async move {
                // We can, however, actually suspend whenever
                // `Function::call_async` is used to call WASM functions.
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                Ok(vec![Value::F64(delta)])
            }
        },
    );

    let import_object = imports! {
        "js" => {
            "init_state" => init_state,
            "compute_delta" => compute_delta,
        }
    };

    let instance = Instance::new(&mut store, &module, &import_object)?;
    let get_state = instance.exports.get_function("get_state")?;
    let update_state = instance.exports.get_function("update_state")?;

    fn as_f64(values: &[Value]) -> f64 {
        match &values[0] {
            Value::F64(v) => *v,
            other => panic!("expected f64 value, got {other:?}"),
        }
    }

    assert_eq!(as_f64(&get_state.call(&mut store, &[])?), 1.0);

    let step = |store: &mut Store, func: &wasmer::Function| -> Result<f64> {
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(func.call_async(store, &[]))?;
        Ok(as_f64(&result))
    };

    assert_eq!(step(&mut store, update_state)?, 1.5);
    assert_eq!(step(&mut store, update_state)?, 0.5);
    assert_eq!(step(&mut store, update_state)?, 3.0);

    Ok(())
}
