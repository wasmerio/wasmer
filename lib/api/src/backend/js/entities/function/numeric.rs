//! Calls that pass nothing but numbers.
//!
//! The general way in and out of JavaScript puts the arguments of a call in a
//! `js_sys::Array` and reads each one back through a `JsValue`: an allocation
//! and several crossings between WebAssembly and JavaScript per argument. A
//! function whose parameters and result are all `i32`, `f32` or `f64` needs
//! none of that: the engine hands such values over as JavaScript numbers,
//! which cross as plain `f64`s (the arguments of a guest function of nothing
//! but `i32`s as `i32`s). This module is that second way, the *numeric
//! lane*, for functions of up to [`MAX_ARITY`] parameters and at most one
//! result.
//!
//! An `i64` is a BigInt in JavaScript, so a signature with one stays on the
//! general path: a lane for it would need a closure type per position of the
//! BigInts. So do references and multiple results.

use std::{mem::MaybeUninit, rc::Rc};

use js_sys::{Function as JsFunction, WebAssembly};
use wasm_bindgen::{
    JsCast, JsValue,
    prelude::{Closure, wasm_bindgen},
};
use wasmer_types::{FunctionType, Type};

use crate::{
    Value,
    js::{store::keep_alive, utils::convert::wasmer_value_to_js},
};

/// The most parameters a function on the numeric lane has: as many as a
/// `wasm_bindgen` closure takes.
pub(crate) const MAX_ARITY: usize = 8;

/// The arguments of a call on the numeric lane, and how many of them count.
pub(crate) type Numbers = ([f64; MAX_ARITY], usize);

fn is_number(ty: &Type) -> bool {
    matches!(ty, Type::I32 | Type::F32 | Type::F64)
}

/// Whether the parameters of functions of type `ty` are passed as numbers:
/// those of an async host function, whose results reach the guest through a
/// promise.
pub(crate) fn takes_numbers(ty: &FunctionType) -> bool {
    ty.params().len() <= MAX_ARITY && ty.params().iter().all(is_number)
}

/// Whether functions of type `ty` are called on the numeric lane.
pub(crate) fn is_numeric(ty: &FunctionType) -> bool {
    ty.params().len() <= MAX_ARITY
        && ty.results().len() <= 1
        && ty.params().iter().chain(ty.results()).all(is_number)
}

/// A number the engine passed for a parameter of type `ty`, or returned.
#[inline]
pub(crate) fn value(ty: &Type, number: f64) -> Value {
    match ty {
        Type::I32 => Value::I32(number as i32),
        Type::F32 => Value::F32(number as f32),
        _ => Value::F64(number),
    }
}

/// The values of a call that arrived as `numbers`, kept on the stack. Only
/// as many as the call has are made: the others stay uninitialized.
pub(crate) struct Values {
    values: [MaybeUninit<Value>; MAX_ARITY],
    len: usize,
}

impl std::ops::Deref for Values {
    type Target = [Value];

    #[inline]
    fn deref(&self) -> &[Value] {
        // SAFETY: `values` initialized the first `len`.
        unsafe { std::slice::from_raw_parts(self.values.as_ptr().cast::<Value>(), self.len) }
    }
}

/// The values of a call that arrived as `numbers`, for parameters of `types`.
#[inline]
pub(crate) fn values(types: &[Type], numbers: &[f64]) -> Values {
    // Making all eight values, as `std::array::from_fn` or a filled array
    // does, costs a dynamic host function about a fifth of its call. The
    // values made are numbers, which own nothing, so that `MaybeUninit`
    // does not drop them loses nothing.
    let mut values = [const { MaybeUninit::uninit() }; MAX_ARITY];
    let mut len = 0;
    for ((slot, ty), number) in values.iter_mut().zip(types).zip(numbers) {
        slot.write(value(ty, *number));
        len += 1;
    }
    Values { values, len }
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

/// The arguments of a call on the numeric lane whose parameters are all
/// i32s, and how many of them count.
pub(crate) type Integers = ([i32; MAX_ARITY], usize);

/// `values` as the i32s to call a function with, if the function `ty` takes
/// nothing but i32s and `values` are as many i32s. `None` otherwise, for
/// [`numbers`] to see to.
#[inline]
pub(crate) fn integers(ty: &FunctionType, values: &[Value]) -> Option<Integers> {
    if values.len() != ty.params().len() || values.is_empty() {
        return None;
    }
    let mut integers = [0; MAX_ARITY];
    for ((integer, value), ty) in integers.iter_mut().zip(values).zip(ty.params()) {
        *integer = match (value, ty) {
            (Value::I32(value), Type::I32) => *value,
            _ => return None,
        };
    }
    Some((integers, values.len()))
}

/// What a host function returned, as the number to hand to the engine: what
/// the general path hands it for a function with one result, where the engine
/// then makes a number of whatever it got. That runs JavaScript for a value
/// of another type, which may throw: what it throws is returned, caught.
pub(crate) fn result(results: &[Value]) -> Result<f64, JsValue> {
    Ok(match &results[0] {
        Value::I32(value) => *value as f64,
        Value::I64(value) => *value as f64,
        Value::F32(value) => *value as f64,
        Value::F64(value) => *value,
        Value::V128(value) => *value as f64,
        other => to_number(&wasmer_value_to_js(other))?,
    })
}

macro_rules! closure {
    ($result:ty; $body:ident; $( $argument:ident ),*) => {
        Closure::wrap(Box::new(move |$( $argument: f64 ),*| $body(&[$( $argument ),*]))
            as Box<dyn Fn($( closure!(@f64 $argument) ),*) -> Result<$result, JsValue>>)
        .into_js_value()
    };
    (@f64 $argument:ident) => { f64 };
}

/// A closure of `arity` numbers that returns a `$result`, as a JavaScript
/// function.
macro_rules! closure_of_arity {
    ($result:ty; $arity:expr, $body:ident) => {{
        let closure = match $arity {
            0 => closure!($result; $body;),
            1 => closure!($result; $body; a),
            2 => closure!($result; $body; a, b),
            3 => closure!($result; $body; a, b, c),
            4 => closure!($result; $body; a, b, c, d),
            5 => closure!($result; $body; a, b, c, d, e),
            6 => closure!($result; $body; a, b, c, d, e, f),
            7 => closure!($result; $body; a, b, c, d, e, f, g),
            8 => closure!($result; $body; a, b, c, d, e, f, g, h),
            _ => unreachable!("the numeric lane ends at {MAX_ARITY} parameters"),
        };
        closure.unchecked_into::<JsFunction>()
    }};
}

/// A JavaScript function of `arity` numbers that calls `body` with them. For
/// a function without a result, what `body` returns is ignored by the engine.
pub(crate) fn closure(
    arity: usize,
    body: impl Fn(&[f64]) -> Result<f64, JsValue> + 'static,
) -> JsFunction {
    closure_of_arity!(f64; arity, body)
}

/// A JavaScript function of `arity` numbers that calls `body` with them, and
/// returns any value: the body of an async host function's import.
#[cfg(feature = "experimental-async")]
pub(crate) fn closure_returning_any(
    arity: usize,
    body: impl Fn(&[f64]) -> Result<JsValue, JsValue> + 'static,
) -> JsFunction {
    closure_of_arity!(JsValue; arity, body)
}

// A `Closure` is a slow function for a guest to call: its glue gathers the
// arguments into an array and spreads them again, counts references around
// the call, and the `Result` it returns comes back as an array the engine
// makes. A dynamic host function is called the way a typed one is instead:
// the guest calls a small JavaScript function, which calls a Rust function of
// the host's table, a *trampoline*, with the address of the host function's
// body and the numbers. The trampoline returns the body's result; an error
// it stashes in a cell, and the JavaScript function throws it from there, so
// the error is thrown by JavaScript as the `Closure` glue throws it, and no
// Rust frame is left behind by a throw.
//
// `address | 0`: the body's address goes to the trampoline's `usize`
// parameter, an i32, which JavaScriptCore takes fast only as an int32 value,
// and the address crossed as an f64 (see `with_leading_arguments`).

/// What a dynamic host function of nothing but numbers runs when a guest
/// calls it: its arguments in, its result or the value to throw out.
type Body = dyn Fn(&[f64]) -> Result<f64, JsValue>;

#[wasm_bindgen(inline_js = r#"
let thrown;
export function stash(error) { thrown = [error]; }
function rethrow() { const error = thrown[0]; thrown = undefined; throw error; }
export function makeHost(trampoline, address, arity) {
  const body = address | 0;
  switch (arity) {
    case 0: return () => {
      const result = trampoline(body);
      if (thrown !== undefined) rethrow();
      return result;
    };
    case 1: return (a) => {
      const result = trampoline(body, a);
      if (thrown !== undefined) rethrow();
      return result;
    };
    case 2: return (a, b) => {
      const result = trampoline(body, a, b);
      if (thrown !== undefined) rethrow();
      return result;
    };
    case 3: return (a, b, c) => {
      const result = trampoline(body, a, b, c);
      if (thrown !== undefined) rethrow();
      return result;
    };
    case 4: return (a, b, c, d) => {
      const result = trampoline(body, a, b, c, d);
      if (thrown !== undefined) rethrow();
      return result;
    };
    case 5: return (a, b, c, d, e) => {
      const result = trampoline(body, a, b, c, d, e);
      if (thrown !== undefined) rethrow();
      return result;
    };
    case 6: return (a, b, c, d, e, f) => {
      const result = trampoline(body, a, b, c, d, e, f);
      if (thrown !== undefined) rethrow();
      return result;
    };
    case 7: return (a, b, c, d, e, f, g) => {
      const result = trampoline(body, a, b, c, d, e, f, g);
      if (thrown !== undefined) rethrow();
      return result;
    };
    case 8: return (a, b, c, d, e, f, g, h) => {
      const result = trampoline(body, a, b, c, d, e, f, g, h);
      if (thrown !== undefined) rethrow();
      return result;
    };
  }
}
"#)]
extern "C" {
    /// The JavaScript function of a host function of `arity` numbers: it
    /// calls `trampoline` with the address of the body it is to call.
    #[wasm_bindgen(js_name = makeHost)]
    fn make_host(trampoline: &JsValue, address: f64, arity: usize) -> JsFunction;

    /// Keeps the value that the JavaScript function of the trampoline that
    /// returns next is to throw.
    fn stash(error: JsValue);
}

macro_rules! trampolines {
    ($( $trampoline:ident ( $( $argument:ident ),* ); )*) => {
        $(
            /// Calls the [`Body`] at `body` with the numbers after it, for
            /// the JavaScript function of a host function made by [`host`].
            ///
            /// # Safety
            ///
            /// `body` is the address of a `Box<Body>` that lives until the
            /// call returns.
            unsafe extern "C" fn $trampoline(body: usize, $( $argument: f64 ),*) -> f64 {
                // SAFETY: the function that passes `body` keeps it alive.
                let body = unsafe { &*(body as *const Box<Body>) };
                match body(&[$( $argument ),*]) {
                    Ok(number) => number,
                    Err(error) => {
                        stash(error);
                        0.0
                    }
                }
            }
        )*
    };
}

trampolines! {
    trampoline0();
    trampoline1(a);
    trampoline2(a, b);
    trampoline3(a, b, c);
    trampoline4(a, b, c, d);
    trampoline5(a, b, c, d, e);
    trampoline6(a, b, c, d, e, f);
    trampoline7(a, b, c, d, e, f, g);
    trampoline8(a, b, c, d, e, f, g, h);
}

/// A host function of `arity` numbers that calls `body` with them, as
/// [`closure`] does, but faster to call. For a function without a result,
/// what `body` returns is ignored by the engine.
pub(crate) fn host(
    arity: usize,
    body: impl Fn(&[f64]) -> Result<f64, JsValue> + 'static,
) -> JsFunction {
    let trampoline = match arity {
        0 => trampoline0 as *const (),
        1 => trampoline1 as *const (),
        2 => trampoline2 as *const (),
        3 => trampoline3 as *const (),
        4 => trampoline4 as *const (),
        5 => trampoline5 as *const (),
        6 => trampoline6 as *const (),
        7 => trampoline7 as *const (),
        8 => trampoline8 as *const (),
        _ => unreachable!("the numeric lane ends at {MAX_ARITY} parameters"),
    };
    // The address of a function is its index in the table.
    let table: WebAssembly::Table = wasm_bindgen::function_table().unchecked_into();
    let trampoline = table.get(trampoline as usize as u32).unwrap();
    let body: Rc<Box<Body>> = Rc::new(Box::new(body));
    let address = Rc::as_ptr(&body) as usize as f64;
    let function = make_host(&trampoline, address, arity);
    keep_alive(body, &function);
    function
}

// A call of a function on the numeric lane goes through a *guard*: a
// function that calls it through a `Reflect.apply` taken when the module is
// loaded, as the general path calls with `Reflect::apply` (nothing is looked
// up on the function), and catches what that throws by itself. Its import is
// not a `catch` one: wasm-bindgen glues those with `handleError`, which
// makes a closure and an `arguments` object on every call and is shared by
// every `catch` import of the program, about half of a call into a guest on
// V8. What it caught waits in a cell for `take`, and the guard says that
// there is something to take by what it returns.
//
// A guard for a function with a result makes a number of what the function
// returns, as WebAssembly makes one of what an import returns, inside the
// `try`: what that throws, for a BigInt, a Symbol or an object whose
// `valueOf` throws, is caught with the call. It returns that number, or
// `NaN` if something threw. A result can be `NaN` too, so a `NaN` only means
// "look in the cell": with nothing there it was the result. A guard for a
// function without a result does not look at what it returns, and returns
// `0`, or `1` if it threw.
//
// Each guard is imported twice, with f64s and with i32s. JavaScriptCore calls
// a WebAssembly function fast only with int32 values for its i32 parameters:
// any other number sends the whole call down a slow path, even a whole one.
// An f64 that crosses into JavaScript is not an int32 value there, an i32
// is, so a function of nothing but i32s is called with i32s.
#[wasm_bindgen(inline_js = r#"
const apply = Reflect.apply;
let caught;
export function take() { const taken = caught; caught = undefined; return taken; }
export function toNumber(value) { return +value; }
export function guard0(target, receiver) { try { return +apply(target, receiver, []); } catch (error) { caught = [error]; return NaN; } }
export function guardVoid0(target, receiver) { try { apply(target, receiver, []); return 0; } catch (error) { caught = [error]; return 1; } }
export function guard1(target, receiver, a) { try { return +apply(target, receiver, [a]); } catch (error) { caught = [error]; return NaN; } }
export function guardVoid1(target, receiver, a) { try { apply(target, receiver, [a]); return 0; } catch (error) { caught = [error]; return 1; } }
export function guard2(target, receiver, a, b) { try { return +apply(target, receiver, [a, b]); } catch (error) { caught = [error]; return NaN; } }
export function guardVoid2(target, receiver, a, b) { try { apply(target, receiver, [a, b]); return 0; } catch (error) { caught = [error]; return 1; } }
export function guard3(target, receiver, a, b, c) { try { return +apply(target, receiver, [a, b, c]); } catch (error) { caught = [error]; return NaN; } }
export function guardVoid3(target, receiver, a, b, c) { try { apply(target, receiver, [a, b, c]); return 0; } catch (error) { caught = [error]; return 1; } }
export function guard4(target, receiver, a, b, c, d) { try { return +apply(target, receiver, [a, b, c, d]); } catch (error) { caught = [error]; return NaN; } }
export function guardVoid4(target, receiver, a, b, c, d) { try { apply(target, receiver, [a, b, c, d]); return 0; } catch (error) { caught = [error]; return 1; } }
export function guard5(target, receiver, a, b, c, d, e) { try { return +apply(target, receiver, [a, b, c, d, e]); } catch (error) { caught = [error]; return NaN; } }
export function guardVoid5(target, receiver, a, b, c, d, e) { try { apply(target, receiver, [a, b, c, d, e]); return 0; } catch (error) { caught = [error]; return 1; } }
export function guard6(target, receiver, a, b, c, d, e, f) { try { return +apply(target, receiver, [a, b, c, d, e, f]); } catch (error) { caught = [error]; return NaN; } }
export function guardVoid6(target, receiver, a, b, c, d, e, f) { try { apply(target, receiver, [a, b, c, d, e, f]); return 0; } catch (error) { caught = [error]; return 1; } }
export function guard7(target, receiver, a, b, c, d, e, f, g) { try { return +apply(target, receiver, [a, b, c, d, e, f, g]); } catch (error) { caught = [error]; return NaN; } }
export function guardVoid7(target, receiver, a, b, c, d, e, f, g) { try { apply(target, receiver, [a, b, c, d, e, f, g]); return 0; } catch (error) { caught = [error]; return 1; } }
export function guard8(target, receiver, a, b, c, d, e, f, g, h) { try { return +apply(target, receiver, [a, b, c, d, e, f, g, h]); } catch (error) { caught = [error]; return NaN; } }
export function guardVoid8(target, receiver, a, b, c, d, e, f, g, h) { try { apply(target, receiver, [a, b, c, d, e, f, g, h]); return 0; } catch (error) { caught = [error]; return 1; } }
"#)]
extern "C" {
    #[wasm_bindgen(catch, js_name = toNumber)]
    fn to_number(value: &JsValue) -> Result<f64, JsValue>;
    /// Empties the cell of the guards: `undefined`, or an array of the one
    /// value a guard caught.
    fn take() -> JsValue;
    fn guard0(function: &JsFunction, receiver: &JsValue) -> f64;
    #[wasm_bindgen(js_name = guardVoid0)]
    fn guard_void0(function: &JsFunction, receiver: &JsValue) -> f64;
    fn guard1(function: &JsFunction, receiver: &JsValue, a: f64) -> f64;
    #[wasm_bindgen(js_name = guard1)]
    fn guard1_i32(function: &JsFunction, receiver: &JsValue, a: i32) -> f64;
    #[wasm_bindgen(js_name = guardVoid1)]
    fn guard_void1(function: &JsFunction, receiver: &JsValue, a: f64) -> f64;
    #[wasm_bindgen(js_name = guardVoid1)]
    fn guard_void1_i32(function: &JsFunction, receiver: &JsValue, a: i32) -> f64;
    fn guard2(function: &JsFunction, receiver: &JsValue, a: f64, b: f64) -> f64;
    #[wasm_bindgen(js_name = guard2)]
    fn guard2_i32(function: &JsFunction, receiver: &JsValue, a: i32, b: i32) -> f64;
    #[wasm_bindgen(js_name = guardVoid2)]
    fn guard_void2(function: &JsFunction, receiver: &JsValue, a: f64, b: f64) -> f64;
    #[wasm_bindgen(js_name = guardVoid2)]
    fn guard_void2_i32(function: &JsFunction, receiver: &JsValue, a: i32, b: i32) -> f64;
    fn guard3(function: &JsFunction, receiver: &JsValue, a: f64, b: f64, c: f64) -> f64;
    #[wasm_bindgen(js_name = guard3)]
    fn guard3_i32(function: &JsFunction, receiver: &JsValue, a: i32, b: i32, c: i32) -> f64;
    #[wasm_bindgen(js_name = guardVoid3)]
    fn guard_void3(function: &JsFunction, receiver: &JsValue, a: f64, b: f64, c: f64) -> f64;
    #[wasm_bindgen(js_name = guardVoid3)]
    fn guard_void3_i32(function: &JsFunction, receiver: &JsValue, a: i32, b: i32, c: i32) -> f64;
    fn guard4(function: &JsFunction, receiver: &JsValue, a: f64, b: f64, c: f64, d: f64) -> f64;
    #[wasm_bindgen(js_name = guard4)]
    fn guard4_i32(function: &JsFunction, receiver: &JsValue, a: i32, b: i32, c: i32, d: i32)
    -> f64;
    #[wasm_bindgen(js_name = guardVoid4)]
    fn guard_void4(
        function: &JsFunction,
        receiver: &JsValue,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
    ) -> f64;
    #[wasm_bindgen(js_name = guardVoid4)]
    fn guard_void4_i32(
        function: &JsFunction,
        receiver: &JsValue,
        a: i32,
        b: i32,
        c: i32,
        d: i32,
    ) -> f64;
    fn guard5(
        function: &JsFunction,
        receiver: &JsValue,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
    ) -> f64;
    #[wasm_bindgen(js_name = guard5)]
    fn guard5_i32(
        function: &JsFunction,
        receiver: &JsValue,
        a: i32,
        b: i32,
        c: i32,
        d: i32,
        e: i32,
    ) -> f64;
    #[wasm_bindgen(js_name = guardVoid5)]
    fn guard_void5(
        function: &JsFunction,
        receiver: &JsValue,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
    ) -> f64;
    #[wasm_bindgen(js_name = guardVoid5)]
    fn guard_void5_i32(
        function: &JsFunction,
        receiver: &JsValue,
        a: i32,
        b: i32,
        c: i32,
        d: i32,
        e: i32,
    ) -> f64;
    #[allow(clippy::too_many_arguments)]
    fn guard6(
        function: &JsFunction,
        receiver: &JsValue,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
        f: f64,
    ) -> f64;
    #[wasm_bindgen(js_name = guard6)]
    #[allow(clippy::too_many_arguments)]
    fn guard6_i32(
        function: &JsFunction,
        receiver: &JsValue,
        a: i32,
        b: i32,
        c: i32,
        d: i32,
        e: i32,
        f: i32,
    ) -> f64;
    #[wasm_bindgen(js_name = guardVoid6)]
    #[allow(clippy::too_many_arguments)]
    fn guard_void6(
        function: &JsFunction,
        receiver: &JsValue,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
        f: f64,
    ) -> f64;
    #[wasm_bindgen(js_name = guardVoid6)]
    #[allow(clippy::too_many_arguments)]
    fn guard_void6_i32(
        function: &JsFunction,
        receiver: &JsValue,
        a: i32,
        b: i32,
        c: i32,
        d: i32,
        e: i32,
        f: i32,
    ) -> f64;
    #[allow(clippy::too_many_arguments)]
    fn guard7(
        function: &JsFunction,
        receiver: &JsValue,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
        f: f64,
        g: f64,
    ) -> f64;
    #[wasm_bindgen(js_name = guard7)]
    #[allow(clippy::too_many_arguments)]
    fn guard7_i32(
        function: &JsFunction,
        receiver: &JsValue,
        a: i32,
        b: i32,
        c: i32,
        d: i32,
        e: i32,
        f: i32,
        g: i32,
    ) -> f64;
    #[wasm_bindgen(js_name = guardVoid7)]
    #[allow(clippy::too_many_arguments)]
    fn guard_void7(
        function: &JsFunction,
        receiver: &JsValue,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
        f: f64,
        g: f64,
    ) -> f64;
    #[wasm_bindgen(js_name = guardVoid7)]
    #[allow(clippy::too_many_arguments)]
    fn guard_void7_i32(
        function: &JsFunction,
        receiver: &JsValue,
        a: i32,
        b: i32,
        c: i32,
        d: i32,
        e: i32,
        f: i32,
        g: i32,
    ) -> f64;
    #[allow(clippy::too_many_arguments)]
    fn guard8(
        function: &JsFunction,
        receiver: &JsValue,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
        f: f64,
        g: f64,
        h: f64,
    ) -> f64;
    #[wasm_bindgen(js_name = guard8)]
    #[allow(clippy::too_many_arguments)]
    fn guard8_i32(
        function: &JsFunction,
        receiver: &JsValue,
        a: i32,
        b: i32,
        c: i32,
        d: i32,
        e: i32,
        f: i32,
        g: i32,
        h: i32,
    ) -> f64;
    #[wasm_bindgen(js_name = guardVoid8)]
    #[allow(clippy::too_many_arguments)]
    fn guard_void8(
        function: &JsFunction,
        receiver: &JsValue,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
        f: f64,
        g: f64,
        h: f64,
    ) -> f64;
    #[wasm_bindgen(js_name = guardVoid8)]
    #[allow(clippy::too_many_arguments)]
    fn guard_void8_i32(
        function: &JsFunction,
        receiver: &JsValue,
        a: i32,
        b: i32,
        c: i32,
        d: i32,
        e: i32,
        f: i32,
        g: i32,
        h: i32,
    ) -> f64;
}

/// What a function returned through its guard, or what it threw. A function
/// without a result returns `NaN`, whatever it returned.
#[inline]
fn returned(returns: bool, number: f64) -> Result<f64, JsValue> {
    let threw = match returns {
        true => number.is_nan(),
        false => number != 0.0,
    };
    match (threw, returns) {
        (true, _) => caught(number),
        (false, true) => Ok(number),
        (false, false) => Ok(f64::NAN),
    }
}

#[cold]
fn caught(number: f64) -> Result<f64, JsValue> {
    let caught = take();
    match caught.is_undefined() {
        true => Ok(number),
        false => Err(caught.unchecked_into::<js_sys::Array>().get(0)),
    }
}

/// The arguments of a call on the numeric lane whose number of parameters is
/// known where it is made: a call of a [`crate::TypedFunction`]. It calls the
/// guard of that many parameters directly, without going through
/// [`Numbers`]. Arguments of nothing but i32s are `[i32; N]`, the others
/// `[f64; N]`.
pub(crate) trait Arguments: Sized {
    /// Calls `function` with `receiver` and these numbers, and returns the
    /// number it returns, or `NaN` without looking at what it returns if
    /// `returns` is false.
    fn call(self, function: &JsFunction, receiver: &JsValue, returns: bool)
    -> Result<f64, JsValue>;
}

macro_rules! arguments {
    ($( $ty:ty, $arity:literal => $guard:ident, $void:ident ( $( $argument:ident ),* ); )*) => {
        $(
            impl Arguments for [$ty; $arity] {
                #[inline]
                fn call(
                    self,
                    function: &JsFunction,
                    receiver: &JsValue,
                    returns: bool,
                ) -> Result<f64, JsValue> {
                    let [$( $argument ),*] = self;
                    let number = match returns {
                        true => $guard(function, receiver, $( $argument ),*),
                        false => $void(function, receiver, $( $argument ),*),
                    };
                    returned(returns, number)
                }
            }
        )*
    };
}

arguments! {
    f64, 0 => guard0, guard_void0();
    f64, 1 => guard1, guard_void1(a);
    f64, 2 => guard2, guard_void2(a, b);
    f64, 3 => guard3, guard_void3(a, b, c);
    f64, 4 => guard4, guard_void4(a, b, c, d);
    f64, 5 => guard5, guard_void5(a, b, c, d, e);
    f64, 6 => guard6, guard_void6(a, b, c, d, e, f);
    f64, 7 => guard7, guard_void7(a, b, c, d, e, f, g);
    f64, 8 => guard8, guard_void8(a, b, c, d, e, f, g, h);
    i32, 0 => guard0, guard_void0();
    i32, 1 => guard1_i32, guard_void1_i32(a);
    i32, 2 => guard2_i32, guard_void2_i32(a, b);
    i32, 3 => guard3_i32, guard_void3_i32(a, b, c);
    i32, 4 => guard4_i32, guard_void4_i32(a, b, c, d);
    i32, 5 => guard5_i32, guard_void5_i32(a, b, c, d, e);
    i32, 6 => guard6_i32, guard_void6_i32(a, b, c, d, e, f);
    i32, 7 => guard7_i32, guard_void7_i32(a, b, c, d, e, f, g);
    i32, 8 => guard8_i32, guard_void8_i32(a, b, c, d, e, f, g, h);
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
    call_with(function, receiver, &numbers[..*arity], returns)
}

/// [`call`] with i32s.
pub(crate) fn call_integers(
    function: &JsFunction,
    receiver: &JsValue,
    (integers, arity): &Integers,
    returns: bool,
) -> Result<f64, JsValue> {
    call_with(function, receiver, &integers[..*arity], returns)
}

#[inline]
fn call_with<T: Copy>(
    function: &JsFunction,
    receiver: &JsValue,
    numbers: &[T],
    returns: bool,
) -> Result<f64, JsValue>
where
    [T; 0]: Arguments,
    [T; 1]: Arguments,
    [T; 2]: Arguments,
    [T; 3]: Arguments,
    [T; 4]: Arguments,
    [T; 5]: Arguments,
    [T; 6]: Arguments,
    [T; 7]: Arguments,
    [T; 8]: Arguments,
{
    let (f, r) = (function, receiver);
    match *numbers {
        [] => <[T; 0]>::call([], f, r, returns),
        [a] => [a].call(f, r, returns),
        [a, b] => [a, b].call(f, r, returns),
        [a, b, c] => [a, b, c].call(f, r, returns),
        [a, b, c, d] => [a, b, c, d].call(f, r, returns),
        [a, b, c, d, e] => [a, b, c, d, e].call(f, r, returns),
        [a, b, c, d, e, g] => [a, b, c, d, e, g].call(f, r, returns),
        [a, b, c, d, e, g, h] => [a, b, c, d, e, g, h].call(f, r, returns),
        [a, b, c, d, e, g, h, i] => [a, b, c, d, e, g, h, i].call(f, r, returns),
        _ => unreachable!("the numeric lane ends at {MAX_ARITY} parameters"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen(inline_js = r#"
    export function testFunctions(thrown) {
      const wrapped = [thrown];
      return [
        (...numbers) => numbers.reduce((sum, number) => sum * 2 + number, 0.5),
        function () { return this === null ? 1 : this === undefined ? 2 : 3; },
        () => NaN,
        () => 0,
        () => 1,
        () => -0,
        () => undefined,
        () => 'text',
        () => 10n,
        () => ({ valueOf() { throw thrown; } }),
        () => { throw thrown; },
        () => { throw undefined; },
        () => { throw null; },
        () => { throw NaN; },
        () => { throw 0; },
        () => { throw 1; },
        () => { throw wrapped; },
      ];
    }
    "#)]
    extern "C" {
        /// Functions that return and throw all kinds of values, `thrown`
        /// among them.
        #[wasm_bindgen(js_name = testFunctions)]
        fn test_functions(thrown: &JsValue) -> js_sys::Array;
    }

    /// What a call returned, comparable: the bits of a number (any `NaN` the
    /// same), or the value thrown (any `TypeError` the same).
    #[derive(Debug)]
    enum Outcome {
        Returned(Option<u64>),
        Threw(JsValue),
    }

    impl PartialEq for Outcome {
        fn eq(&self, other: &Self) -> bool {
            let type_error = |value: &JsValue| value.is_instance_of::<js_sys::TypeError>();
            match (self, other) {
                (Self::Returned(a), Self::Returned(b)) => a == b,
                (Self::Threw(a), Self::Threw(b)) => {
                    js_sys::Object::is(a, b) || (type_error(a) && type_error(b))
                }
                _ => false,
            }
        }
    }

    fn outcome(returned: Result<f64, JsValue>, returns: bool) -> Outcome {
        match returned {
            // What a function without a result returns is not used.
            Ok(_) if !returns => Outcome::Returned(None),
            Ok(number) if number.is_nan() => Outcome::Returned(Some(u64::MAX)),
            Ok(number) => Outcome::Returned(Some(number.to_bits())),
            Err(thrown) => Outcome::Threw(thrown),
        }
    }

    /// What a call through `Reflect.apply` returns, made a number as the
    /// engine makes one of the result of an import, inside the call.
    fn reference(
        function: &JsFunction,
        receiver: &JsValue,
        numbers: &[f64],
        returns: bool,
    ) -> Result<f64, JsValue> {
        let arguments: js_sys::Array = numbers.iter().map(|n| JsValue::from_f64(*n)).collect();
        let value = js_sys::Reflect::apply(function, receiver, &arguments)?;
        match returns {
            true => to_number(&value),
            false => Ok(f64::NAN),
        }
    }

    /// Through a guard, a call returns and throws what it does through
    /// `Reflect.apply`, whatever the function returns or throws, with the
    /// receiver it is given, however many parameters it has, and a call
    /// after one that threw is not taken for one that threw. So does a call
    /// with i32s through the guard of i32s, against the same numbers as f64s.
    #[wasm_bindgen_test]
    fn a_guard_returns_and_throws_as_reflect_apply_does() {
        let thrown = JsValue::from(js_sys::Object::new());
        let functions = test_functions(&thrown);
        let returns_nan: JsFunction = functions.get(2).unchecked_into();
        let numbers = [1.5, -2.0, 3.25, 0.0, -0.0, 1e300, -7.0, 8.0];
        let integers = [7, -1, i32::MIN, 0, i32::MAX, 1, -123_456_789, 42];
        for function in functions.iter() {
            let function: JsFunction = function.unchecked_into();
            for receiver in [JsValue::NULL, JsValue::UNDEFINED] {
                for arity in 0..=MAX_ARITY {
                    for returns in [true, false] {
                        let guarded = call(&function, &receiver, &copy(&numbers[..arity]), returns);
                        let expected = reference(&function, &receiver, &numbers[..arity], returns);
                        assert_eq!(
                            outcome(guarded, returns),
                            outcome(expected, returns),
                            "{function:?} of {arity}"
                        );

                        let mut list = [0; MAX_ARITY];
                        list[..arity].copy_from_slice(&integers[..arity]);
                        let guarded = call_integers(&function, &receiver, &(list, arity), returns);
                        let as_f64 = integers.map(f64::from);
                        let expected = reference(&function, &receiver, &as_f64[..arity], returns);
                        assert_eq!(
                            outcome(guarded, returns),
                            outcome(expected, returns),
                            "{function:?} of {arity} i32s"
                        );

                        let next = call(&returns_nan, &receiver, &copy(&[]), true);
                        assert_eq!(outcome(next, true), Outcome::Returned(Some(u64::MAX)));
                    }
                }
            }
        }
    }

    /// A host function returns and throws what a [`closure`] of the same
    /// body does, with the arguments in the same order, and a host function
    /// that returns after one that threw returns.
    #[wasm_bindgen_test]
    fn a_host_function_returns_and_throws_as_a_closure_does() {
        let thrown = JsValue::from(js_sys::Object::new());
        let returning = |number: f64| -> Rc<Body> { Rc::new(move |_| Ok(number)) };
        let throwing = |value: JsValue| -> Rc<Body> { Rc::new(move |_| Err(value.clone())) };
        let bodies = [
            Rc::new(|numbers: &[f64]| Ok(numbers.iter().fold(0.5, |sum, n| sum * 2.0 + n))),
            returning(f64::NAN),
            returning(-0.0),
            returning(f64::INFINITY),
            throwing(thrown),
            throwing(JsValue::UNDEFINED),
            throwing(JsValue::NULL),
            throwing(JsValue::from(f64::NAN)),
            throwing(JsValue::from(0)),
        ];
        let call = |function: &JsFunction, arity: usize| {
            let numbers = [1.5, -2.0, 3.25, 0.0, -0.0, 1e300, -7.0, 8.0];
            let arguments: js_sys::Array =
                numbers[..arity].iter().map(|n| JsValue::from(*n)).collect();
            match js_sys::Reflect::apply(function, &JsValue::UNDEFINED, &arguments) {
                Ok(returned) => Outcome::Returned(returned.as_f64().map(|n| match n.is_nan() {
                    true => u64::MAX,
                    false => n.to_bits(),
                })),
                Err(thrown) => Outcome::Threw(thrown),
            }
        };
        let after = host(0, |_| Ok(1.0));
        for arity in 0..=MAX_ARITY {
            for body in &bodies {
                let (made, wrapped) = (body.clone(), body.clone());
                let made = host(arity, move |numbers| made(numbers));
                let wrapped = closure(arity, move |numbers| wrapped(numbers));
                assert_eq!(made.length() as usize, arity);
                assert_eq!(call(&made, arity), call(&wrapped, arity), "of {arity}");
                assert_eq!(call(&after, 0), Outcome::Returned(Some(1.0f64.to_bits())));
            }
        }
    }
}
