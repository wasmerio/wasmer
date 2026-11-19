//! Numbers on their way between host and guest: every `i32`, `f32` and `f64`
//! arrives as it was sent, whatever the number of parameters and whichever
//! way the function is called. The `js` backend passes such values without
//! the array of arguments it uses for other types, for up to eight
//! parameters, so the tests go past that on both sides.
#![cfg(any(feature = "sys", feature = "js"))]

use std::sync::{Arc, Mutex};

use macro_wasmer_engine_test::engine_test;
#[cfg(feature = "js")]
use wasm_bindgen_test::*;
use wasmer::*;

/// A value as its bits, so that `-0.0` is not `0.0` and a NaN is a NaN.
fn bits(value: &Value) -> (Type, u64) {
    match value {
        Value::I32(value) => (Type::I32, *value as u32 as u64),
        Value::F32(value) if value.is_nan() => (Type::F32, u64::MAX),
        Value::F32(value) => (Type::F32, value.to_bits() as u64),
        Value::F64(value) if value.is_nan() => (Type::F64, u64::MAX),
        Value::F64(value) => (Type::F64, value.to_bits()),
        Value::I64(value) => (Type::I64, *value as u64),
        other => panic!("{other:?} is not a number"),
    }
}

/// The module of a text fixture, parsed here so that the tests do not need the
/// library's `wat` feature.
fn module(store: &impl AsEngineRef, text: &str) -> Module {
    Module::new(store, wat::parse_str(text).unwrap()).unwrap()
}

fn all_bits(values: &[Value]) -> Vec<(Type, u64)> {
    values.iter().map(bits).collect()
}

fn wat_types(types: &[Type]) -> String {
    let names = types.iter().map(|ty| ty.to_string().to_lowercase());
    names.collect::<Vec<_>>().join(" ")
}

/// What a host function was called with, call by call.
type Calls = Arc<Mutex<Vec<Vec<(Type, u64)>>>>;

/// A guest whose export `call` passes its arguments to the import `host` and
/// returns what that returns. `host` is a dynamic host function of that type
/// which records its arguments and returns `results(arguments)`.
fn guest(
    store: &mut Store,
    ty: &FunctionType,
    results: impl Fn(&[Value]) -> Vec<Value> + Send + Sync + 'static,
) -> (Function, Calls) {
    let (params, result) = (wat_types(ty.params()), wat_types(ty.results()));
    let arguments: String = (0..ty.params().len())
        .map(|index| format!("(local.get {index}) "))
        .collect();
    let wat = format!(
        r#"(module
          (import "host" "host" (func $host (param {params}) (result {result})))
          (func (export "call") (param {params}) (result {result}) (call $host {arguments})))"#
    );
    let module = module(&store, &wat);
    let calls = Calls::default();
    let host = Function::new(store, ty.clone(), {
        let calls = calls.clone();
        move |arguments| {
            calls.lock().unwrap().push(all_bits(arguments));
            Ok(results(arguments))
        }
    });
    let instance = Instance::new(store, &module, &imports! { "host" => { "host" => host } });
    let instance = instance.unwrap();
    let call = instance.exports.get_function("call").unwrap();
    (call.clone(), calls)
}

fn i32s() -> Vec<Value> {
    let values = [0, 1, -1, i32::MIN, i32::MAX, 0x1234_5678, -123_456_789];
    values.into_iter().map(Value::I32).collect()
}

fn f32s() -> Vec<Value> {
    let values = [
        0.0,
        -0.0,
        0.1,
        -1.5,
        16_777_217.0,
        f32::EPSILON,
        f32::MIN_POSITIVE,
        f32::MIN_POSITIVE / 4.0,
        f32::MAX,
        f32::MIN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
    ];
    values.into_iter().map(Value::F32).collect()
}

fn f64s() -> Vec<Value> {
    let values = [
        0.0,
        -0.0,
        0.1,
        -1.5,
        9_007_199_254_740_993.0,
        4_294_967_296.0,
        f64::EPSILON,
        f64::MIN_POSITIVE,
        5e-324,
        f64::MAX,
        f64::MIN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
    ];
    values.into_iter().map(Value::F64).collect()
}

/// Every kind of number, through a dynamic host function and back, called
/// with `Function::call`.
#[engine_test]
fn numbers_arrive_as_they_were_sent() -> Result<(), String> {
    for (ty, values) in [
        (Type::I32, i32s()),
        (Type::F32, f32s()),
        (Type::F64, f64s()),
    ] {
        let mut store = Store::default();
        let signature = FunctionType::new([ty], [ty]);
        let (call, calls) = guest(&mut store, &signature, |arguments| arguments.to_vec());
        for value in values {
            let returned = call.call(&mut store, std::slice::from_ref(&value)).unwrap();
            assert_eq!(bits(&returned[0]), bits(&value), "{value:?} came back");
            let received = calls.lock().unwrap().pop().unwrap();
            assert_eq!(received, [bits(&value)], "{value:?} reached the host");
        }
    }
    Ok(())
}

/// The same with `TypedFunction::call`.
#[engine_test]
fn numbers_arrive_as_they_were_sent_by_a_typed_call() -> Result<(), String> {
    let mut store = Store::default();
    let signature = FunctionType::new([Type::I32, Type::F32, Type::F64], [Type::F64]);
    let (call, calls) = guest(&mut store, &signature, |arguments| {
        vec![arguments[2].clone()]
    });
    let call: TypedFunction<(i32, f32, f64), f64> = call.typed(&store).unwrap();
    let values = i32s().into_iter().cycle().zip(f32s()).zip(f64s());
    for ((a, b), c) in values {
        let returned = call
            .call(&mut store, a.unwrap_i32(), b.unwrap_f32(), c.unwrap_f64())
            .unwrap();
        assert_eq!(bits(&Value::F64(returned)), bits(&c));
        let received = calls.lock().unwrap().pop().unwrap();
        assert_eq!(received, all_bits(&[a, b, c]));
    }

    for (ty, values) in [(Type::I32, i32s()), (Type::F32, f32s())] {
        let signature = FunctionType::new([ty], [ty]);
        let (call, _) = guest(&mut store, &signature, |arguments| arguments.to_vec());
        for value in values {
            let returned = match &value {
                Value::I32(value) => {
                    let call: TypedFunction<i32, i32> = call.typed(&store).unwrap();
                    Value::I32(call.call(&mut store, *value).unwrap())
                }
                Value::F32(value) => {
                    let call: TypedFunction<f32, f32> = call.typed(&store).unwrap();
                    Value::F32(call.call(&mut store, *value).unwrap())
                }
                _ => unreachable!(),
            };
            assert_eq!(bits(&returned), bits(&value));
        }
    }
    Ok(())
}

/// An `i32` the guest makes from an unsigned constant is the same negative
/// number to the host, and comes back as the same bits.
#[engine_test]
fn an_i32_keeps_its_bits() -> Result<(), String> {
    let mut store = Store::default();
    let module = module(
        &store,
        r#"(module
          (import "host" "host" (func $host (param i32) (result i32)))
          (func (export "high") (result i32) (call $host (i32.const 0x80000000)))
          (func (export "ones") (result i32) (call $host (i32.const 0xffffffff)))
          (func (export "shifted") (param i32) (result i32)
            (i32.shr_u (call $host (i32.shl (local.get 0) (i32.const 31))) (i32.const 31))))"#,
    );
    let seen = Arc::new(Mutex::new(Vec::new()));
    let host = Function::new(&mut store, FunctionType::new([Type::I32], [Type::I32]), {
        let seen = seen.clone();
        move |arguments| {
            seen.lock().unwrap().push(arguments[0].unwrap_i32());
            Ok(arguments.to_vec())
        }
    });
    let instance = Instance::new(
        &mut store,
        &module,
        &imports! { "host" => { "host" => host } },
    );
    let exports = instance.unwrap().exports;
    let mut call = |name: &str, arguments: &[Value]| {
        let function = exports.get_function(name).unwrap();
        function.call(&mut store, arguments).unwrap()[0].unwrap_i32()
    };
    assert_eq!(call("high", &[]), i32::MIN);
    assert_eq!(call("ones", &[]), -1);
    assert_eq!(call("shifted", &[Value::I32(1)]), 1);
    assert_eq!(*seen.lock().unwrap(), [i32::MIN, -1, i32::MIN]);
    Ok(())
}

/// From no parameter to more than the `js` backend passes as plain numbers,
/// of mixed types, with a result and without one.
#[engine_test]
fn any_number_of_parameters() -> Result<(), String> {
    let kinds = [Type::I32, Type::F32, Type::F64];
    for arity in 0..=10 {
        let params: Vec<Type> = kinds.iter().copied().cycle().take(arity).collect();
        let arguments: Vec<Value> = (0..arity)
            .map(|index| match params[index] {
                Type::I32 => Value::I32(-(index as i32) - 1),
                Type::F32 => Value::F32(index as f32 + 0.5),
                _ => Value::F64(-(index as f64) * 1e100),
            })
            .collect();
        for results in [vec![], vec![Type::F64]] {
            let mut store = Store::default();
            let signature = FunctionType::new(params.clone(), results.clone());
            let has_result = !results.is_empty();
            let (call, calls) = guest(&mut store, &signature, move |arguments| {
                let last = arguments.last().cloned().unwrap_or(Value::I32(7));
                let last = match last {
                    Value::I32(value) => value as f64,
                    Value::F32(value) => value as f64,
                    value => value.unwrap_f64(),
                };
                vec![Value::F64(last * 2.0); has_result as usize]
            });
            let returned = call.call(&mut store, &arguments).unwrap();
            assert_eq!(
                *calls.lock().unwrap(),
                [all_bits(&arguments)],
                "{signature}"
            );
            assert_eq!(returned.len(), results.len(), "{signature}");
            if has_result {
                let last = match arguments.last() {
                    None => 7.0,
                    Some(Value::I32(value)) => *value as f64,
                    Some(Value::F32(value)) => *value as f64,
                    Some(value) => value.unwrap_f64(),
                };
                assert_eq!(returned[0].unwrap_f64(), last * 2.0, "{signature}");
            }
        }
    }
    Ok(())
}

/// Typed calls at the same edges: no parameter, eight, and nine.
#[engine_test]
fn any_number_of_parameters_in_a_typed_call() -> Result<(), String> {
    use Type::{F32, F64, I32};
    let mut store = Store::default();
    let sum = |arguments: &[Value]| {
        let numbers = arguments.iter().map(|value| match value {
            Value::I32(value) => *value as f64,
            Value::F32(value) => *value as f64,
            value => value.unwrap_f64(),
        });
        vec![Value::F64(numbers.sum())]
    };

    let (call, _) = guest(&mut store, &FunctionType::new([], [F64]), sum);
    let call: TypedFunction<(), f64> = call.typed(&store).unwrap();
    assert_eq!(call.call(&mut store).unwrap(), 0.0);

    let eight = [I32, F32, F64, I32, F32, F64, I32, F32];
    let (call, calls) = guest(&mut store, &FunctionType::new(eight, [F64]), sum);
    type Eight = (i32, f32, f64, i32, f32, f64, i32, f32);
    let call: TypedFunction<Eight, f64> = call.typed(&store).unwrap();
    let returned = call.call(&mut store, -1, 0.5, 1e10, i32::MIN, -0.25, 3.0, 7, 2.0);
    assert_eq!(returned.unwrap(), 1e10 - 2147483648.0 + 11.25);
    assert_eq!(calls.lock().unwrap()[0][3], (I32, i32::MIN as u32 as u64));

    let nine = [I32, F32, F64, I32, F32, F64, I32, F32, F64];
    let (call, calls) = guest(&mut store, &FunctionType::new(nine, [F64]), sum);
    type Nine = (i32, f32, f64, i32, f32, f64, i32, f32, f64);
    let call: TypedFunction<Nine, f64> = call.typed(&store).unwrap();
    let returned = call.call(
        &mut store,
        -1,
        0.5,
        1e10,
        i32::MIN,
        -0.25,
        3.0,
        7,
        2.0,
        -0.0,
    );
    assert_eq!(returned.unwrap(), 1e10 - 2147483648.0 + 11.25);
    assert_eq!(calls.lock().unwrap()[0][8], (F64, (-0.0f64).to_bits()));

    // Without a result.
    let (call, calls) = guest(&mut store, &FunctionType::new([I32, F64], []), |_| vec![]);
    let call: TypedFunction<(i32, f64), ()> = call.typed(&store).unwrap();
    call.call(&mut store, -5, f64::INFINITY).unwrap();
    let received = all_bits(&[Value::I32(-5), Value::F64(f64::INFINITY)]);
    assert_eq!(*calls.lock().unwrap(), [received]);
    Ok(())
}

/// Types that are not plain numbers to JavaScript go the way they always
/// did, alone and next to numbers.
#[engine_test]
fn other_types_are_not_affected() -> Result<(), String> {
    let mut store = Store::default();
    let signature = FunctionType::new([Type::I32, Type::I64, Type::F64], [Type::F64]);
    let (call, calls) = guest(&mut store, &signature, |arguments| {
        vec![arguments[2].clone()]
    });
    let arguments = [Value::I32(-1), Value::I64(i64::MIN + 1), Value::F64(-0.0)];
    let returned = call.call(&mut store, &arguments).unwrap();
    assert_eq!(bits(&returned[0]), bits(&arguments[2]));
    assert_eq!(*calls.lock().unwrap(), [all_bits(&arguments)]);

    let module = module(
        &store,
        r#"(module
          (func (export "i64") (param i64) (result i64) (local.get 0))
          (func (export "both") (param i32 f64) (result f64 i32) (local.get 1) (local.get 0)))"#,
    );
    let exports = Instance::new(&mut store, &module, &imports! {})
        .unwrap()
        .exports;
    for value in [0, -1, i64::MIN, i64::MAX, 1 << 53] {
        let identity: TypedFunction<i64, i64> = exports.get_typed_function(&store, "i64").unwrap();
        assert_eq!(identity.call(&mut store, value).unwrap(), value);
        let identity = exports.get_function("i64").unwrap();
        let returned = identity.call(&mut store, &[Value::I64(value)]).unwrap();
        assert_eq!(returned[0].unwrap_i64(), value);
    }
    let both = exports.get_function("both").unwrap();
    let returned = both.call(&mut store, &[Value::I32(-3), Value::F64(0.5)]);
    assert_eq!(
        all_bits(&returned.unwrap()),
        all_bits(&[Value::F64(0.5), Value::I32(-3)])
    );
    Ok(())
}

/// A reference, which is not a plain number either, goes the way it always
/// did.
#[engine_test]
#[cfg_attr(
    feature = "v8-default",
    ignore = "extern refs are not supported by the default v8 backend"
)]
fn references_are_not_affected() -> Result<(), String> {
    let mut store = Store::default();
    let module = module(
        &store,
        r#"(module
          (func (export "reference") (param externref) (result externref) (local.get 0)))"#,
    );
    let exports = Instance::new(&mut store, &module, &imports! {})
        .unwrap()
        .exports;
    let reference = ExternRef::new(&mut store, 99u32);
    let identity = exports.get_function("reference").unwrap();
    let returned = identity.call(&mut store, &[Value::ExternRef(Some(reference))]);
    match &returned.unwrap()[0] {
        Value::ExternRef(Some(reference)) => {
            assert_eq!(reference.downcast::<u32>(&store), Some(&99))
        }
        other => panic!("not the reference that was passed: {other:?}"),
    }
    Ok(())
}

/// A dynamic host function can be reached again while it runs, through a
/// guest it calls: one that takes nothing but plain numbers, and one that
/// does not.
#[engine_test]
fn a_dynamic_host_function_can_reenter_its_guest() -> Result<(), String> {
    struct Env(Option<Function>);
    // A function is not `Send` on every backend; the test has one thread.
    unsafe impl Send for Env {}
    for (extra_type, extra_argument) in [("", ""), ("i64", "(i64.const 5)")] {
        let mut store = Store::default();
        let wat = format!(
            r#"(module
              (import "host" "down" (func $down (param i32 {extra_type}) (result i32)))
              (func (export "down") (param i32) (result i32)
                (call $down (local.get 0) {extra_argument})))"#
        );
        let module = module(&store, &wat);
        let env = FunctionEnv::new(&mut store, Env(None));
        let mut params = vec![Type::I32];
        params.extend((!extra_type.is_empty()).then_some(Type::I64));
        let signature = FunctionType::new(params, [Type::I32]);
        let down = Function::new_with_env(&mut store, &env, signature, |mut env, arguments| {
            let depth = arguments[0].unwrap_i32();
            if depth == 0 {
                return Ok(vec![Value::I32(0)]);
            }
            let down = env.data().0.clone().unwrap();
            let below = down.call(&mut env, &[Value::I32(depth - 1)])?;
            Ok(vec![Value::I32(below[0].unwrap_i32() + depth)])
        });
        let instance = Instance::new(
            &mut store,
            &module,
            &imports! { "host" => { "down" => down } },
        );
        let instance = instance.unwrap();
        let down = instance.exports.get_function("down").unwrap().clone();
        env.as_mut(&mut store).0 = Some(down.clone());
        let returned = down.call(&mut store, &[Value::I32(4)]).unwrap();
        assert_eq!(
            returned[0].unwrap_i32(),
            10,
            "extra parameter: {extra_type:?}"
        );
    }
    Ok(())
}

/// A dynamic host function is trusted with its results. What the `js`
/// backend does with wrong ones is the same whether the function passes its
/// values as plain numbers (the first signature) or not (the second).
#[cfg(feature = "js")]
mod wrong_results {
    use super::*;

    fn signatures() -> [FunctionType; 2] {
        [
            FunctionType::new([Type::I32], [Type::I32]),
            FunctionType::new([Type::I32, Type::I64], [Type::I32]),
        ]
    }

    fn call_returning(signature: &FunctionType, results: Vec<Value>) -> i32 {
        let mut store = Store::default();
        let (call, _) = guest(&mut store, signature, move |_| results.clone());
        let arguments = [Value::I32(1), Value::I64(2)];
        let arguments = &arguments[..signature.params().len()];
        call.call(&mut store, arguments).unwrap()[0].unwrap_i32()
    }

    #[wasm_bindgen_test]
    fn results_of_another_type_are_converted_by_the_engine() {
        let cases = [
            (vec![Value::F64(2.9)], 2),
            (vec![Value::F64(-2.9)], -2),
            (vec![Value::F64(f64::NAN)], 0),
            (vec![Value::F64(4_294_967_297.0)], 1),
            (vec![Value::F32(7.5)], 7),
            (vec![Value::I64(-7)], -7),
            (vec![Value::I32(5), Value::I32(6)], 5),
        ];
        for (results, expected) in cases {
            for signature in signatures() {
                assert_eq!(
                    call_returning(&signature, results.clone()),
                    expected,
                    "{signature} returning {results:?}"
                );
            }
        }
    }

    #[wasm_bindgen_test]
    #[should_panic(expected = "index out of bounds")]
    fn a_missing_result_panics_with_plain_numbers() {
        call_returning(&signatures()[0], vec![]);
    }

    #[wasm_bindgen_test]
    #[should_panic(expected = "index out of bounds")]
    fn a_missing_result_panics_without_them() {
        call_returning(&signatures()[1], vec![]);
    }
}

/// JavaScript functions given to `wasmer` as they are, through `AsJs`, take
/// the same lane when their signature has only numbers, and are called as the
/// other lane calls them: without looking anything up on them, with the
/// receiver of that lane (`null` for `Function::call`, `undefined` for a
/// typed call), and without looking at what they return when the signature
/// has no result.
#[cfg(feature = "js")]
mod javascript_functions {
    use wasm_bindgen::{JsValue, prelude::wasm_bindgen};
    use wasm_bindgen_test::wasm_bindgen_test;
    use wasmer::{js::AsJs, *};

    #[wasm_bindgen(inline_js = r#"
    export function receiverOf() {
      return function () { return this === null ? 1 : this === undefined ? 2 : 3; };
    }
    export function returningASymbol() {
      return function () { return Symbol(); };
    }
    export function withItsOwnCall(f) {
      Object.defineProperty(f, "call", { value() { throw new Error("its own call"); } });
      return f;
    }
    "#)]
    extern "C" {
        #[wasm_bindgen(js_name = receiverOf)]
        fn receiver_of() -> JsValue;
        #[wasm_bindgen(js_name = returningASymbol)]
        fn returning_a_symbol() -> JsValue;
        #[wasm_bindgen(js_name = withItsOwnCall)]
        fn with_its_own_call(f: JsValue) -> JsValue;
    }

    fn function(store: &mut Store, ty: FunctionType, value: &JsValue) -> Function {
        Function::from_jsvalue(store, &ty, value).unwrap()
    }

    #[wasm_bindgen_test]
    fn they_get_the_receiver_of_their_lane() {
        let mut store = Store::default();
        let ty = FunctionType::new([], [Type::I32]);
        for value in [receiver_of(), with_its_own_call(receiver_of())] {
            let f = function(&mut store, ty.clone(), &value);
            let result = f.call(&mut store, &[]).unwrap();
            assert_eq!(result[0].unwrap_i32(), 1, "Function::call passes null");
            let typed: TypedFunction<(), i32> = f.typed(&store).unwrap();
            assert_eq!(
                typed.call(&mut store).unwrap(),
                2,
                "a typed call passes undefined"
            );
        }
    }

    #[wasm_bindgen_test]
    fn what_they_return_without_a_result_is_ignored() {
        let mut store = Store::default();
        let f = function(
            &mut store,
            FunctionType::new([Type::I32], []),
            &returning_a_symbol(),
        );
        assert!(f.call(&mut store, &[Value::I32(1)]).unwrap().is_empty());
        let typed: TypedFunction<i32, ()> = f.typed(&store).unwrap();
        typed.call(&mut store, 1).unwrap();
    }
}
