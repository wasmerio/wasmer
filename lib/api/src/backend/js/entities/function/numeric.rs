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

#[wasm_bindgen]
extern "C" {
    /// A function called through `Function.prototype.call` with numbers. What
    /// it returns crosses as a number too: `NaN` if it returns nothing.
    type NumericFunction;
    #[wasm_bindgen(method, catch, js_name = call)]
    fn call0(this: &NumericFunction, receiver: &JsValue) -> Result<f64, JsValue>;
    #[wasm_bindgen(method, catch, js_name = call)]
    fn call1(this: &NumericFunction, receiver: &JsValue, a: f64) -> Result<f64, JsValue>;
    #[wasm_bindgen(method, catch, js_name = call)]
    fn call2(this: &NumericFunction, receiver: &JsValue, a: f64, b: f64) -> Result<f64, JsValue>;
    #[wasm_bindgen(method, catch, js_name = call)]
    fn call3(
        this: &NumericFunction,
        receiver: &JsValue,
        a: f64,
        b: f64,
        c: f64,
    ) -> Result<f64, JsValue>;
    #[wasm_bindgen(method, catch, js_name = call)]
    fn call4(
        this: &NumericFunction,
        receiver: &JsValue,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
    ) -> Result<f64, JsValue>;
    #[wasm_bindgen(method, catch, js_name = call)]
    fn call5(
        this: &NumericFunction,
        receiver: &JsValue,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
    ) -> Result<f64, JsValue>;
    #[wasm_bindgen(method, catch, js_name = call)]
    #[allow(clippy::too_many_arguments)]
    fn call6(
        this: &NumericFunction,
        receiver: &JsValue,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
        f: f64,
    ) -> Result<f64, JsValue>;
    #[wasm_bindgen(method, catch, js_name = call)]
    #[allow(clippy::too_many_arguments)]
    fn call7(
        this: &NumericFunction,
        receiver: &JsValue,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
        f: f64,
        g: f64,
    ) -> Result<f64, JsValue>;
    #[wasm_bindgen(method, catch, js_name = call)]
    #[allow(clippy::too_many_arguments)]
    fn call8(
        this: &NumericFunction,
        receiver: &JsValue,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
        f: f64,
        g: f64,
        h: f64,
    ) -> Result<f64, JsValue>;
}

/// Calls `function` with `numbers`, and returns the number it returns.
pub(crate) fn call(function: &JsFunction, (numbers, arity): &Numbers) -> Result<f64, JsValue> {
    let function: &NumericFunction = function.unchecked_ref();
    let receiver = &JsValue::UNDEFINED;
    match numbers[..*arity] {
        [] => function.call0(receiver),
        [a] => function.call1(receiver, a),
        [a, b] => function.call2(receiver, a, b),
        [a, b, c] => function.call3(receiver, a, b, c),
        [a, b, c, d] => function.call4(receiver, a, b, c, d),
        [a, b, c, d, e] => function.call5(receiver, a, b, c, d, e),
        [a, b, c, d, e, f] => function.call6(receiver, a, b, c, d, e, f),
        [a, b, c, d, e, f, g] => function.call7(receiver, a, b, c, d, e, f, g),
        [a, b, c, d, e, f, g, h] => function.call8(receiver, a, b, c, d, e, f, g, h),
        _ => unreachable!("the numeric lane ends at {MAX_ARITY} parameters"),
    }
}
