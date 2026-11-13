// With `v8-default` (the Windows test lane) `Store::default()` is a V8 store,
// which checks the results of a host function in its own way.
#![cfg(all(
    feature = "sys",
    not(feature = "v8-default"),
    not(target_arch = "wasm32")
))]
//! What a dynamic host function returns is checked against the type the
//! function was created with, on every call.

use wasmer::{Function, FunctionType, RuntimeError, Store, Type, Value};

fn i32_to_i32() -> FunctionType {
    FunctionType::new(vec![Type::I32], vec![Type::I32])
}

fn assert_wrong_signature(what: &str, result: Result<Box<[Value]>, RuntimeError>) {
    let message = result.unwrap_err().message();
    assert!(message.contains("wrong signature"), "{what}: {message}");
}

#[test]
fn a_dynamic_function_returns_what_its_type_says() {
    let mut store = Store::default();
    let right = Function::new(&mut store, i32_to_i32(), |args| {
        Ok(vec![Value::I32(args[0].unwrap_i32() + 1)])
    });
    let wrong_type = Function::new(&mut store, i32_to_i32(), |_| Ok(vec![Value::I64(1)]));
    let too_few = Function::new(&mut store, i32_to_i32(), |_| Ok(vec![]));
    let too_many = Function::new(&mut store, i32_to_i32(), |_| {
        Ok(vec![Value::I32(1), Value::I32(2)])
    });

    let args = [Value::I32(7)];
    for _ in 0..2 {
        assert_eq!(right.call(&mut store, &args).unwrap()[0].unwrap_i32(), 8);
        assert_wrong_signature("wrong type", wrong_type.call(&mut store, &args));
        assert_wrong_signature("too few", too_few.call(&mut store, &args));
        assert_wrong_signature("too many", too_many.call(&mut store, &args));
    }
}

#[cfg(feature = "experimental-async")]
#[test]
fn an_async_dynamic_function_returns_what_its_type_says() {
    let mut store = Store::default();
    let right = Function::new_async(&mut store, i32_to_i32(), |args| {
        let value = args[0].unwrap_i32();
        async move { Ok(vec![Value::I32(value + 1)]) }
    });
    let wrong_type = Function::new_async(&mut store, i32_to_i32(), |_| async {
        Ok(vec![Value::I64(1)])
    });
    let too_few = Function::new_async(&mut store, i32_to_i32(), |_| async { Ok(vec![]) });
    let typed = Function::new_typed_async(&mut store, async |value: i32| value + 1);

    // Called synchronously, where a ready future completes in place...
    let args = [Value::I32(7)];
    assert_eq!(right.call(&mut store, &args).unwrap()[0].unwrap_i32(), 8);
    assert_wrong_signature("wrong type", wrong_type.call(&mut store, &args));
    assert_wrong_signature("too few", too_few.call(&mut store, &args));

    // ...and on a coroutine.
    let store = store.into_async();
    let call = |function: &Function| {
        futures::executor::block_on(function.call_async(&store, vec![Value::I32(7)]))
    };
    for _ in 0..2 {
        assert_eq!(call(&right).unwrap()[0].unwrap_i32(), 8);
        assert_eq!(call(&typed).unwrap()[0].unwrap_i32(), 8);
        assert_wrong_signature("wrong type", call(&wrong_type));
        assert_wrong_signature("too few", call(&too_few));
    }
}
