use macro_wasmer_engine_test::engine_test;
#[cfg(feature = "js")]
use wasm_bindgen_test::wasm_bindgen_test;

use wasmer::*;

#[engine_test]
fn reentrant_host_calls_preserve_store_access() -> Result<(), String> {
    #[derive(Default)]
    struct Env {
        callback: Option<TypedFunction<i32, i32>>,
        visits: i32,
    }

    fn host(mut env: FunctionEnvMut<Env>, depth: i32) -> Result<i32, RuntimeError> {
        env.data_mut().visits += 1;
        let result = if depth == 0 {
            0
        } else {
            let callback = env.data().callback.clone().unwrap();
            callback.call(&mut env, depth - 1)?
        };
        // The nested call must not invalidate this frame's store borrow.
        env.data_mut().visits += 1;
        Ok(result + 1)
    }

    for dynamic in [false, true] {
        let mut store = Store::default();
        let env = FunctionEnv::new(&mut store, Env::default());
        let function = if dynamic {
            Function::new_with_env(
                &mut store,
                &env,
                FunctionType::new([Type::I32], [Type::I32]),
                |env, args| host(env, args[0].unwrap_i32()).map(|value| vec![Value::I32(value)]),
            )
        } else {
            Function::new_typed_with_env(&mut store, &env, host)
        };
        // Host contexts point to the allocation, not the movable Store handle.
        let mut store = Box::new(store);
        let module = Module::new(
            &store,
            r#"(module
                (func $host (import "host" "run") (param i32) (result i32))
                (func (export "run") (param i32) (result i32)
                    (call $host (local.get 0)))
                (func $start (drop (call $host (i32.const 0))))
                (start $start))"#,
        )
        .map_err(|e| format!("{e:?}"))?;
        let imports = imports! { "host" => { "run" => function } };
        let instance =
            Instance::new(&mut store, &module, &imports).map_err(|e| format!("{e:?}"))?;
        assert_eq!(env.as_ref(&store).visits, 2);
        let callback = instance
            .exports
            .get_typed_function::<i32, i32>(&store, "run")
            .map_err(|e| format!("{e:?}"))?;
        env.as_mut(&mut store).callback = Some(callback.clone());
        assert_eq!(
            callback.call(&mut store, 3).map_err(|e| format!("{e:?}"))?,
            4
        );
        assert_eq!(env.as_ref(&store).visits, 10);
    }
    Ok(())
}

#[engine_test]
fn data_and_store_mut() -> Result<(), String> {
    let mut store = Store::default();
    let global_mut = Global::new_mut(&mut store, Value::I32(10));
    struct Env {
        value: i32,
        global: Global,
    }
    let env = FunctionEnv::new(
        &mut store,
        Env {
            value: 0i32,
            global: global_mut,
        },
    );
    let mut envmut = env.into_mut(&mut store);

    let (data, mut storemut) = envmut.data_and_store_mut();

    assert_eq!(
        data.global.ty(&storemut),
        GlobalType {
            ty: Type::I32,
            mutability: Mutability::Var
        }
    );
    assert_eq!(data.global.get(&mut storemut), Value::I32(10));
    data.value = data.global.get(&mut storemut).unwrap_i32() + 10;

    data.global
        .set(&mut storemut, Value::I32(data.value))
        .unwrap();

    assert_eq!(data.global.get(&mut storemut), Value::I32(data.value));

    Ok(())
}
