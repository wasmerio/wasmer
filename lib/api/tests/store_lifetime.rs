//! A host function belongs to the store it was made in, and runs only while
//! that store is alive. On the `js` backend a host function is a JavaScript
//! function that outlives its store if something still holds it, such as the
//! export of a guest that imports it.

use macro_wasmer_engine_test::engine_test;
#[cfg(feature = "js")]
use wasm_bindgen_test::*;
use wasmer::*;

/// A guest whose export `call` calls the import `host.f` of type `ty`.
fn guest(store: &Store, ty: Type) -> Module {
    let ty = ty.to_string().to_lowercase();
    let wat = format!(
        r#"(module
            (import "host" "f" (func $f (param {ty}) (result {ty})))
            (func (export "call") (param {ty}) (result {ty}) (call $f (local.get 0))))"#
    );
    Module::new(store, wat).unwrap()
}

/// One host function of each kind, all of `ty -> ty`, made in `store`. Those
/// with an environment add its value to their argument.
fn host_functions(store: &mut Store, ty: Type) -> Vec<(&'static str, Function)> {
    let env = FunctionEnv::new(store, 1000);
    let signature = FunctionType::new([ty], [ty]);
    let add_env = |env: &FunctionEnvMut<i32>, value: &Value| match value {
        Value::I32(value) => Value::I32(value + env.data()),
        Value::I64(value) => Value::I64(value + *env.data() as i64),
        other => panic!("{other:?}"),
    };
    let mut functions = vec![
        (
            "dynamic",
            Function::new(store, signature.clone(), |values| Ok(values.to_vec())),
        ),
        (
            "dynamic with an environment",
            Function::new_with_env(store, &env, signature.clone(), move |env, values| {
                Ok(vec![add_env(&env, &values[0])])
            }),
        ),
    ];
    if ty == Type::I32 {
        functions.push(("typed", Function::new_typed(store, |value: i32| value)));
        functions.push((
            "typed with an environment",
            Function::new_typed_with_env(store, &env, |env: FunctionEnvMut<i32>, value: i32| {
                value + env.data()
            }),
        ));
    } else {
        functions.push(("typed", Function::new_typed(store, |value: i64| value)));
        functions.push((
            "typed with an environment",
            Function::new_typed_with_env(store, &env, |env: FunctionEnvMut<i32>, value: i64| {
                value + *env.data() as i64
            }),
        ));
    }
    functions
}

#[cfg(feature = "js")]
fn argument(ty: Type) -> Value {
    match ty {
        Type::I32 => Value::I32(7),
        _ => Value::I64(7),
    }
}

#[engine_test]
fn a_host_function_is_only_from_the_store_it_was_made_in() -> Result<(), String> {
    // `i64` takes the general path of the `js` backend, the others the
    // numeric lane.
    for ty in [Type::I32, Type::I64] {
        let mut store = Store::default();
        let mut other = Store::default();
        let module = guest(&other, ty);
        for (kind, function) in host_functions(&mut store, ty) {
            assert!(function.is_from_store(&store), "{kind} {ty}");
            assert!(!function.is_from_store(&other), "{kind} {ty}");
            let imports = imports! { "host" => { "f" => function } };
            let instance = Instance::new(&mut other, &module, &imports);
            assert!(instance.is_err(), "{kind} {ty}");
        }
    }
    Ok(())
}

/// Whether `message` is the error of a host function that refused to run: on
/// `js` a host function runs only with the store the call it runs in lends
/// it, which is its own only in a call made with its own store, and so never
/// once that store is dropped.
#[cfg(feature = "js")]
fn refused(message: &str) -> bool {
    message.contains("its own store")
}

/// On `js`, a guest can be called with any store, and it then calls its
/// imports whatever their store: a host function whose store was dropped
/// must fail the call instead of using that store.
#[cfg(feature = "js")]
#[wasm_bindgen_test]
fn a_host_function_of_a_dropped_store_fails_the_call() {
    let mut failures = vec![];
    for ty in [Type::I32, Type::I64] {
        let mut store = Store::default();
        let module = guest(&store, ty);
        let mut calls = vec![];
        for (kind, function) in host_functions(&mut store, ty) {
            let imports = imports! { "host" => { "f" => function } };
            let instance = Instance::new(&mut store, &module, &imports).unwrap();
            let call = instance.exports.get_function("call").unwrap().clone();
            assert!(
                call.call(&mut store, &[argument(ty)]).is_ok(),
                "{kind} {ty}"
            );
            calls.push((kind, call));
        }
        drop(store);

        // Likely where the dropped store was, with an environment where the
        // dropped one had its own.
        let mut other = Store::default();
        let _env = FunctionEnv::new(&mut other, 2000);
        for (kind, call) in calls {
            match call.call(&mut other, &[argument(ty)]) {
                Ok(results) => {
                    failures.push(format!("{kind} {ty} ran without its store: {results:?}"))
                }
                Err(error) if !refused(&error.message()) => {
                    failures.push(format!("{kind} {ty}: {}", error.message()))
                }
                Err(_) => {}
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[cfg(feature = "js")]
mod while_dropped {
    use std::cell::RefCell;

    use super::*;

    thread_local! {
        static OTHER: RefCell<Option<Store>> = const { RefCell::new(None) };
        static RESULTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    /// Calls the guest's exports from its `Drop`, with another store.
    struct CallOnDrop(Vec<(&'static str, Type, Function)>);

    impl Drop for CallOnDrop {
        fn drop(&mut self) {
            // Likely takes the place of an environment dropped just before.
            let filler = std::hint::black_box(Box::new(2000i32));
            for (kind, ty, call) in self.0.drain(..) {
                let result = OTHER.with(|other| {
                    call.call(other.borrow_mut().as_mut().unwrap(), &[argument(ty)])
                        .map_err(|error| error.message())
                });
                if !matches!(&result, Err(message) if refused(message)) {
                    RESULTS.with(|results| {
                        results
                            .borrow_mut()
                            .push(format!("{kind} {ty}: {result:?}"))
                    });
                }
            }
            drop(filler);
        }
    }

    /// On `js` a guest can be called with any store, also from the `Drop` of
    /// a value in an environment of a store that is being dropped. A host
    /// function of that store, which the guest calls, must fail the call
    /// as it does once the store is gone, rather than use the store's
    /// environments, which may be gone already.
    #[wasm_bindgen_test]
    fn a_host_function_called_while_its_store_is_dropped_fails_the_call() {
        OTHER.with(|other| *other.borrow_mut() = Some(Store::default()));
        let mut store = Store::default();
        let mut calls = vec![];
        for ty in [Type::I32, Type::I64] {
            let module = guest(&store, ty);
            for (kind, function) in host_functions(&mut store, ty) {
                let imports = imports! { "host" => { "f" => function } };
                let instance = Instance::new(&mut store, &module, &imports).unwrap();
                let call = instance.exports.get_function("call").unwrap().clone();
                assert!(
                    call.call(&mut store, &[argument(ty)]).is_ok(),
                    "{kind} {ty}"
                );
                calls.push((kind, ty, call));
            }
        }
        // Made after the environments of the host functions, so dropped
        // after them.
        let _call_on_drop = FunctionEnv::new(&mut store, CallOnDrop(calls));
        drop(store);
        OTHER.with(|other| other.borrow_mut().take());
        let failures = RESULTS.with(|results| std::mem::take(&mut *results.borrow_mut()));
        assert!(failures.is_empty(), "{failures:#?}");
    }
}

/// On `js` a host function that a guest exports again, as its own export, is
/// a function of no store in particular, like any function of a guest; a call
/// of it after its store was dropped still fails.
#[cfg(feature = "js")]
#[wasm_bindgen_test]
fn a_host_function_exported_again_by_a_guest_fails_after_its_store_is_dropped() {
    let mut store = Store::default();
    let mut other = Store::default();
    let module = Module::new(
        &store,
        r#"(module (import "host" "f" (func (param i32) (result i32))) (export "f" (func 0)))"#,
    )
    .unwrap();
    let mut failures = vec![];
    for (kind, function) in host_functions(&mut store, Type::I32) {
        assert!(!function.is_from_store(&other), "{kind}");
        let imports = imports! { "host" => { "f" => function } };
        let instance = Instance::new(&mut store, &module, &imports).unwrap();
        let exported = instance.exports.get_function("f").unwrap().clone();
        assert!(exported.is_from_store(&other), "{kind}");
        failures.push((kind, exported));
    }
    drop(store);
    let _env = FunctionEnv::new(&mut other, 2000);
    let failures: Vec<_> = failures
        .into_iter()
        .filter_map(
            |(kind, exported)| match exported.call(&mut other, &[Value::I32(7)]) {
                Err(error) if refused(&error.message()) => None,
                result => Some(format!("{kind}: {:?}", result.map_err(|e| e.message()))),
            },
        )
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}
