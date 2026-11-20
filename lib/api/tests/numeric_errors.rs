//! A call that fails fails the same way whether it passes plain numbers or
//! not, and leaves nothing behind for the next call: one that returns `NaN`
//! right after a failed one returns it. The `js` backend calls functions of
//! nothing but numbers its own way, so these tests cover each result type of
//! such functions, called both ways.

use macro_wasmer_engine_test::engine_test;
#[cfg(feature = "js")]
use wasm_bindgen_test::*;
use wasmer::*;

/// The result types a function of plain numbers can have.
const RESULTS: [Option<Type>; 4] = [None, Some(Type::I32), Some(Type::F32), Some(Type::F64)];

fn wat_result(result: Option<Type>) -> String {
    match result {
        Some(ty) => format!("(result {})", ty.to_string().to_lowercase()),
        None => String::new(),
    }
}

/// What a function of `result` returns when it does not fail: `NaN` where
/// it can, so that a `NaN` after a failed call is seen as the result it is.
fn wat_returned(result: Option<Type>) -> &'static str {
    match result {
        None => "",
        Some(Type::I32) => "(i32.const 7)",
        Some(Type::F32) => "(f32.const nan)",
        Some(_) => "(f64.const nan)",
    }
}

/// Whether `values` are what a call that did not fail returns.
fn returned(result: Option<Type>, values: &[Value]) -> bool {
    match (result, values) {
        (None, []) => true,
        (Some(Type::I32), [Value::I32(7)]) => true,
        (Some(Type::F32), [Value::F32(value)]) => value.is_nan(),
        (Some(Type::F64), [Value::F64(value)]) => value.is_nan(),
        _ => false,
    }
}

/// Calls `function` with `fail` as its one argument, typed with the result
/// type of `result`, and returns what it returned as values.
fn typed_call(
    store: &mut Store,
    function: &Function,
    result: Option<Type>,
    fail: i32,
) -> Result<Vec<Value>, RuntimeError> {
    Ok(match result {
        None => {
            let () = function
                .typed::<i32, ()>(store)
                .unwrap()
                .call(store, fail)?;
            vec![]
        }
        Some(Type::I32) => {
            let typed = function.typed::<i32, i32>(store).unwrap();
            vec![Value::I32(typed.call(store, fail)?)]
        }
        Some(Type::F32) => {
            let typed = function.typed::<i32, f32>(store).unwrap();
            vec![Value::F32(typed.call(store, fail)?)]
        }
        Some(_) => {
            let typed = function.typed::<i32, f64>(store).unwrap();
            vec![Value::F64(typed.call(store, fail)?)]
        }
    })
}

/// Calls `function` with `fail` both ways, twice, failing and not, and checks
/// what each call returns. `failed` checks the error of a failed call.
fn check_calls(
    store: &mut Store,
    function: &Function,
    result: Option<Type>,
    failed: impl Fn(RuntimeError),
) {
    for _ in 0..2 {
        failed(typed_call(store, function, result, 1).unwrap_err());
        let values = typed_call(store, function, result, 0).unwrap();
        assert!(returned(result, &values), "{result:?}: {values:?}");

        failed(function.call(store, &[Value::I32(1)]).unwrap_err());
        let values = function.call(store, &[Value::I32(0)]).unwrap();
        assert!(returned(result, &values), "{result:?}: {values:?}");
    }
}

/// A guest that traps fails the call.
#[engine_test]
fn a_trap_fails_the_call() -> Result<(), String> {
    for result in RESULTS {
        let mut store = Store::default();
        let wat = format!(
            r#"(module
              (func (export "call") (param i32) {}
                (if (local.get 0) (then unreachable))
                {}))"#,
            wat_result(result),
            wat_returned(result),
        );
        let module = Module::new(&store, wat).unwrap();
        let instance = Instance::new(&mut store, &module, &imports! {}).unwrap();
        let call = instance.exports.get_function("call").unwrap().clone();
        check_calls(&mut store, &call, result, |_error| {});
    }
    Ok(())
}

#[derive(Debug)]
struct HostError;

impl std::fmt::Display for HostError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("the host function failed")
    }
}

impl std::error::Error for HostError {}

/// What a host function that does not fail returns for `result`.
fn host_returned(result: Option<Type>) -> Vec<Value> {
    match result {
        None => vec![],
        Some(Type::I32) => vec![Value::I32(7)],
        Some(Type::F32) => vec![Value::F32(f32::NAN)],
        Some(_) => vec![Value::F64(f64::NAN)],
    }
}

/// A guest whose export `call` calls the import `host.f`, both of `i32` to
/// `result`.
fn guest(store: &mut Store, result: Option<Type>, host: Function) -> Function {
    let wat = format!(
        r#"(module
          (import "host" "f" (func $f (param i32) {0}))
          (func (export "call") (param i32) {0} (call $f (local.get 0))))"#,
        wat_result(result),
    );
    let module = Module::new(&store, wat).unwrap();
    let imports = imports! { "host" => { "f" => host } };
    let instance = Instance::new(store, &module, &imports).unwrap();
    instance.exports.get_function("call").unwrap().clone()
}

/// A dynamic host function that fails fails the call of its guest with its
/// error.
#[engine_test]
fn a_dynamic_host_function_fails_the_call() -> Result<(), String> {
    for result in RESULTS {
        let mut store = Store::default();
        let signature = FunctionType::new([Type::I32], result.into_iter().collect::<Vec<_>>());
        let host = Function::new(&mut store, signature, move |arguments| {
            match arguments[0].unwrap_i32() {
                0 => Ok(host_returned(result)),
                _ => Err(RuntimeError::new("the host function failed")),
            }
        });
        let call = guest(&mut store, result, host);
        check_calls(&mut store, &call, result, |error| {
            assert!(
                error.message().contains("the host function failed"),
                "{error:?}"
            );
        });
    }
    Ok(())
}

/// A typed host function that fails fails the call of its guest with its
/// error, which the caller gets back as it was.
#[engine_test]
fn a_typed_host_function_fails_the_call() -> Result<(), String> {
    fn fail(fail: i32) -> Result<(), HostError> {
        match fail {
            0 => Ok(()),
            _ => Err(HostError),
        }
    }
    for result in RESULTS {
        let mut store = Store::default();
        let host = match result {
            None => Function::new_typed(&mut store, |value: i32| fail(value)),
            Some(Type::I32) => {
                Function::new_typed(&mut store, |value: i32| fail(value).map(|()| 7))
            }
            Some(Type::F32) => {
                Function::new_typed(&mut store, |value: i32| fail(value).map(|()| f32::NAN))
            }
            Some(_) => Function::new_typed(&mut store, |value: i32| fail(value).map(|()| f64::NAN)),
        };
        let call = guest(&mut store, result, host);
        check_calls(&mut store, &call, result, |error| {
            assert!(error.downcast::<HostError>().is_ok());
        });
    }
    Ok(())
}
