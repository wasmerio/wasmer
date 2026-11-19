//! Calls that pass nothing but numbers.
//!
//! The general way in and out of JavaScript puts the arguments of a call in a
//! `js_sys::Array` and reads each one back through a `JsValue`: an allocation
//! and several crossings between WebAssembly and JavaScript per argument. A
//! function whose parameters and result are all `i32`, `f32` or `f64` needs
//! none of that: the engine hands such values over as JavaScript numbers,
//! which cross as plain `f64`s. This module is that second way, the *numeric
//! lane*, for functions of up to [`MAX_ARITY`] parameters and at most one
//! result.
//!
//! An `i64` is a BigInt in JavaScript, so a signature with one stays on the
//! general path: a lane for it would need a closure type per position of the
//! BigInts. So do references and multiple results.

use js_sys::Function as JsFunction;
use wasm_bindgen::{
    JsCast, JsValue,
    prelude::{Closure, wasm_bindgen},
};
use wasmer_types::{FunctionType, Type};

use crate::{Value, js::utils::convert::wasmer_value_to_js};

/// The most parameters a function on the numeric lane has: as many as a
/// `wasm_bindgen` closure takes.
pub(crate) const MAX_ARITY: usize = 8;

/// The arguments of a call on the numeric lane, and how many of them count.
pub(crate) type Numbers = ([f64; MAX_ARITY], usize);

fn is_number(ty: &Type) -> bool {
    matches!(ty, Type::I32 | Type::F32 | Type::F64)
}

/// Whether functions of type `ty` are called on the numeric lane.
pub(crate) fn is_numeric(ty: &FunctionType) -> bool {
    ty.params().len() <= MAX_ARITY
        && ty.results().len() <= 1
        && ty.params().iter().chain(ty.results()).all(is_number)
}

/// A number the engine passed for a parameter of type `ty`, or returned.
pub(crate) fn value(ty: &Type, number: f64) -> Value {
    match ty {
        Type::I32 => Value::I32(number as i32),
        Type::F32 => Value::F32(number as f32),
        _ => Value::F64(number),
    }
}

/// The values of a call that arrived as `numbers`, for parameters of `types`.
pub(crate) fn values(types: &[Type], numbers: &[f64]) -> [Value; MAX_ARITY] {
    std::array::from_fn(|index| match types.get(index) {
        Some(ty) => value(ty, numbers[index]),
        None => Value::I32(0),
    })
}

/// The arguments of a call that arrived as `numbers`, to keep.
pub(crate) fn copy(numbers: &[f64]) -> Numbers {
    let mut copy = [0.0; MAX_ARITY];
    copy[..numbers.len()].copy_from_slice(numbers);
    (copy, numbers.len())
}

/// `values` as the numbers to call a function with. `None` if one of them is
/// not a number, or if there are not as many as the function `ty` takes: the
/// general path then does with them what it always did.
pub(crate) fn numbers(ty: &FunctionType, values: &[Value]) -> Option<Numbers> {
    if values.len() != ty.params().len() {
        return None;
    }
    let mut numbers = [0.0; MAX_ARITY];
    for (number, value) in numbers.iter_mut().zip(values) {
        *number = match value {
            Value::I32(value) => *value as f64,
            Value::F32(value) => *value as f64,
            Value::F64(value) => *value,
            _ => return None,
        };
    }
    Some((numbers, values.len()))
}

/// What a host function returned, as the number to hand to the engine: what
/// the general path hands it for a function with one result, where the engine
/// then makes a number of whatever it got.
pub(crate) fn result(results: &[Value]) -> f64 {
    match &results[0] {
        Value::I32(value) => *value as f64,
        Value::I64(value) => *value as f64,
        Value::F32(value) => *value as f64,
        Value::F64(value) => *value,
        Value::V128(value) => *value as f64,
        other => wasmer_value_to_js(other).unchecked_into_f64(),
    }
}

macro_rules! closure {
    ($body:ident; $( $argument:ident ),*) => {
        Closure::wrap(Box::new(move |$( $argument: f64 ),*| $body(&[$( $argument ),*]))
            as Box<dyn Fn($( closure!(@f64 $argument) ),*) -> Result<f64, JsValue>>)
        .into_js_value()
    };
    (@f64 $argument:ident) => { f64 };
}

/// A JavaScript function of `arity` numbers that calls `body` with them. For
/// a function without a result, what `body` returns is ignored by the engine.
pub(crate) fn closure(
    arity: usize,
    body: impl Fn(&[f64]) -> Result<f64, JsValue> + 'static,
) -> JsFunction {
    let closure = match arity {
        0 => closure!(body;),
        1 => closure!(body; a),
        2 => closure!(body; a, b),
        3 => closure!(body; a, b, c),
        4 => closure!(body; a, b, c, d),
        5 => closure!(body; a, b, c, d, e),
        6 => closure!(body; a, b, c, d, e, f),
        7 => closure!(body; a, b, c, d, e, f, g),
        8 => closure!(body; a, b, c, d, e, f, g, h),
        _ => unreachable!("the numeric lane ends at {MAX_ARITY} parameters"),
    };
    closure.unchecked_into()
}

// Calls through a `Reflect.apply` taken when the module is loaded, as the
// generic path calls with `Reflect::apply`: nothing is looked up on the
// function. What it returns crosses as a number; a call with no result does
// not look at it.
#[wasm_bindgen(inline_js = r#"
const apply = Reflect.apply;
export function call0(target, r) { return apply(target, r, []); }
export function callVoid0(target, r) { apply(target, r, []); }
export function call1(target, r, a) { return apply(target, r, [a]); }
export function callVoid1(target, r, a) { apply(target, r, [a]); }
export function call2(target, r, a, b) { return apply(target, r, [a, b]); }
export function callVoid2(target, r, a, b) { apply(target, r, [a, b]); }
export function call3(target, r, a, b, c) { return apply(target, r, [a, b, c]); }
export function callVoid3(target, r, a, b, c) { apply(target, r, [a, b, c]); }
export function call4(target, r, a, b, c, d) { return apply(target, r, [a, b, c, d]); }
export function callVoid4(target, r, a, b, c, d) { apply(target, r, [a, b, c, d]); }
export function call5(target, r, a, b, c, d, e) { return apply(target, r, [a, b, c, d, e]); }
export function callVoid5(target, r, a, b, c, d, e) { apply(target, r, [a, b, c, d, e]); }
export function call6(target, r, a, b, c, d, e, f) { return apply(target, r, [a, b, c, d, e, f]); }
export function callVoid6(target, r, a, b, c, d, e, f) { apply(target, r, [a, b, c, d, e, f]); }
export function call7(target, r, a, b, c, d, e, f, g) { return apply(target, r, [a, b, c, d, e, f, g]); }
export function callVoid7(target, r, a, b, c, d, e, f, g) { apply(target, r, [a, b, c, d, e, f, g]); }
export function call8(target, r, a, b, c, d, e, f, g, h) { return apply(target, r, [a, b, c, d, e, f, g, h]); }
export function callVoid8(target, r, a, b, c, d, e, f, g, h) { apply(target, r, [a, b, c, d, e, f, g, h]); }
"#)]
extern "C" {
    #[wasm_bindgen(catch)]
    fn call0(target: &JsFunction, receiver: &JsValue) -> Result<f64, JsValue>;
    #[wasm_bindgen(catch, js_name = callVoid0)]
    fn call_void0(target: &JsFunction, receiver: &JsValue) -> Result<(), JsValue>;
    #[wasm_bindgen(catch)]
    fn call1(target: &JsFunction, receiver: &JsValue, a: f64) -> Result<f64, JsValue>;
    #[wasm_bindgen(catch, js_name = callVoid1)]
    fn call_void1(target: &JsFunction, receiver: &JsValue, a: f64) -> Result<(), JsValue>;
    #[wasm_bindgen(catch)]
    fn call2(target: &JsFunction, receiver: &JsValue, a: f64, b: f64) -> Result<f64, JsValue>;
    #[wasm_bindgen(catch, js_name = callVoid2)]
    fn call_void2(target: &JsFunction, receiver: &JsValue, a: f64, b: f64) -> Result<(), JsValue>;
    #[wasm_bindgen(catch)]
    fn call3(target: &JsFunction, receiver: &JsValue, a: f64, b: f64, c: f64) -> Result<f64, JsValue>;
    #[wasm_bindgen(catch, js_name = callVoid3)]
    fn call_void3(target: &JsFunction, receiver: &JsValue, a: f64, b: f64, c: f64) -> Result<(), JsValue>;
    #[wasm_bindgen(catch)]
    fn call4(target: &JsFunction, receiver: &JsValue, a: f64, b: f64, c: f64, d: f64) -> Result<f64, JsValue>;
    #[wasm_bindgen(catch, js_name = callVoid4)]
    fn call_void4(target: &JsFunction, receiver: &JsValue, a: f64, b: f64, c: f64, d: f64) -> Result<(), JsValue>;
    #[wasm_bindgen(catch)]
    fn call5(target: &JsFunction, receiver: &JsValue, a: f64, b: f64, c: f64, d: f64, e: f64) -> Result<f64, JsValue>;
    #[wasm_bindgen(catch, js_name = callVoid5)]
    fn call_void5(target: &JsFunction, receiver: &JsValue, a: f64, b: f64, c: f64, d: f64, e: f64) -> Result<(), JsValue>;
    #[wasm_bindgen(catch)]
    #[allow(clippy::too_many_arguments)]
    fn call6(target: &JsFunction, receiver: &JsValue, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Result<f64, JsValue>;
    #[wasm_bindgen(catch, js_name = callVoid6)]
    #[allow(clippy::too_many_arguments)]
    fn call_void6(target: &JsFunction, receiver: &JsValue, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Result<(), JsValue>;
    #[wasm_bindgen(catch)]
    #[allow(clippy::too_many_arguments)]
    fn call7(target: &JsFunction, receiver: &JsValue, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, g: f64) -> Result<f64, JsValue>;
    #[wasm_bindgen(catch, js_name = callVoid7)]
    #[allow(clippy::too_many_arguments)]
    fn call_void7(target: &JsFunction, receiver: &JsValue, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, g: f64) -> Result<(), JsValue>;
    #[wasm_bindgen(catch)]
    #[allow(clippy::too_many_arguments)]
    fn call8(target: &JsFunction, receiver: &JsValue, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, g: f64, h: f64) -> Result<f64, JsValue>;
    #[wasm_bindgen(catch, js_name = callVoid8)]
    #[allow(clippy::too_many_arguments)]
    fn call_void8(target: &JsFunction, receiver: &JsValue, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, g: f64, h: f64) -> Result<(), JsValue>;
}

/// Calls `function` with `receiver` and `numbers`, and returns the number it
/// returns, or `NaN` without looking at what it returns if `returns` is
/// false.
pub(crate) fn call(
    function: &JsFunction,
    receiver: &JsValue,
    (numbers, arity): &Numbers,
    returns: bool,
) -> Result<f64, JsValue> {
    match (&numbers[..*arity], returns) {
        (&[], true) => call0(function, receiver),
        (&[], false) => call_void0(function, receiver).map(|()| f64::NAN),
        (&[a], true) => call1(function, receiver, a),
        (&[a], false) => call_void1(function, receiver, a).map(|()| f64::NAN),
        (&[a, b], true) => call2(function, receiver, a, b),
        (&[a, b], false) => call_void2(function, receiver, a, b).map(|()| f64::NAN),
        (&[a, b, c], true) => call3(function, receiver, a, b, c),
        (&[a, b, c], false) => call_void3(function, receiver, a, b, c).map(|()| f64::NAN),
        (&[a, b, c, d], true) => call4(function, receiver, a, b, c, d),
        (&[a, b, c, d], false) => call_void4(function, receiver, a, b, c, d).map(|()| f64::NAN),
        (&[a, b, c, d, e], true) => call5(function, receiver, a, b, c, d, e),
        (&[a, b, c, d, e], false) => call_void5(function, receiver, a, b, c, d, e).map(|()| f64::NAN),
        (&[a, b, c, d, e, f], true) => call6(function, receiver, a, b, c, d, e, f),
        (&[a, b, c, d, e, f], false) => call_void6(function, receiver, a, b, c, d, e, f).map(|()| f64::NAN),
        (&[a, b, c, d, e, f, g], true) => call7(function, receiver, a, b, c, d, e, f, g),
        (&[a, b, c, d, e, f, g], false) => call_void7(function, receiver, a, b, c, d, e, f, g).map(|()| f64::NAN),
        (&[a, b, c, d, e, f, g, h], true) => call8(function, receiver, a, b, c, d, e, f, g, h),
        (&[a, b, c, d, e, f, g, h], false) => call_void8(function, receiver, a, b, c, d, e, f, g, h).map(|()| f64::NAN),
        _ => unreachable!("the numeric lane ends at {MAX_ARITY} parameters"),
    }
}
