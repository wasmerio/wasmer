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

/// What a dynamic host function of nothing but numbers runs when a guest
/// calls it: its arguments in, its result or the value to throw out.
type Body = dyn Fn(&[f64]) -> Result<f64, JsValue>;

#[wasm_bindgen]
extern "C" {
    /// Makes the JavaScript function of a host function: called with a
    /// trampoline and the address of the body it is to call.
    type Maker;
    #[wasm_bindgen(method, js_name = call)]
    fn make(this: &Maker, receiver: &JsValue, trampoline: &JsValue, body: f64) -> JsFunction;

    /// Keeps the value that the JavaScript function of the trampoline that
    /// returns next is to throw.
    type Stash;
    #[wasm_bindgen(method, js_name = call)]
    fn stash(this: &Stash, receiver: &JsValue, error: JsValue);
}

/// The makers of host functions of this thread, by number of parameters, and
/// the stash they share.
struct Hosts {
    makers: [Maker; MAX_ARITY + 1],
    stash: Stash,
}

impl Hosts {
    /// The makers, if the host lets them be made.
    fn new() -> Option<Self> {
        let makers = (0..=MAX_ARITY).map(|arity| {
            let parameters = (0..arity)
                .map(|index| format!("a{index}"))
                .collect::<Vec<_>>();
            let arguments = [vec!["body".to_owned()], parameters.clone()].concat();
            format!(
                "(trampoline, body) => ({}) => {{ const result = trampoline({}); \
                 if (thrown !== undefined) {{ const error = thrown[0]; thrown = undefined; throw error; }} \
                 return result; }}",
                parameters.join(", "),
                arguments.join(", "),
            )
        });
        let body = format!(
            "\"use strict\"; let thrown; return [[{}], (error) => {{ thrown = [error]; }}];",
            makers.collect::<Vec<_>>().join(", "),
        );
        let make: JsFunction = super::EvaluatedFunction::new("", &body)
            .ok()?
            .unchecked_into();
        let made: js_sys::Array = make.call0(&JsValue::UNDEFINED).ok()?.unchecked_into();
        let makers: js_sys::Array = made.get(0).unchecked_into();
        Some(Self {
            makers: std::array::from_fn(|arity| makers.get(arity as u32).unchecked_into()),
            stash: made.get(1).unchecked_into(),
        })
    }
}

thread_local! {
    static HOSTS: Option<Hosts> = Hosts::new();
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

/// Gives `error` to the JavaScript function of the trampoline that returns
/// next, to throw.
#[cold]
fn stash(error: JsValue) {
    HOSTS.with(|hosts| match hosts {
        Some(hosts) => hosts.stash.stash(&JsValue::UNDEFINED, error),
        // A trampoline is only called by a function that `Hosts` made.
        None => unreachable!("a host function without its makers"),
    })
}

/// A host function of `arity` numbers that calls `body` with them, as
/// [`closure`] does, but faster to call. For a function without a result,
/// what `body` returns is ignored by the engine.
pub(crate) fn host(
    arity: usize,
    body: impl Fn(&[f64]) -> Result<f64, JsValue> + 'static,
) -> JsFunction {
    HOSTS.with(|hosts| {
        let Some(hosts) = hosts else {
            return closure(arity, body);
        };
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
        let function = hosts.makers[arity].make(&JsValue::UNDEFINED, &trampoline, address);
        keep_alive("wasmer.host", body, &function);
        function
    })
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

// The `catch` imports above are what wasm-bindgen glues with `handleError`,
// which makes a closure and an `arguments` object on every call and is shared
// by every `catch` import of the program: on V8 that is about half of a call
// into a guest. A *guard* below does the same work in a function of its own:
// it calls the function it is given as `this` with its arguments, and catches
// what that throws by itself. Its import is not a `catch` one, so its glue is
// a plain call. What it caught waits in a cell for `take`, and the guard says
// that there is something to take by what it returns.
//
// A guard for a function with a result returns what the function returns, or
// `NaN` if it threw. A result can be `NaN` too, so a `NaN` only means "look in
// the cell": with nothing there it was the result. A guard for a function
// without a result returns `0`, or `1` if it threw.
//
// The guards are made from text, which a host can forbid; the `catch` imports
// are the way there.
#[wasm_bindgen]
extern "C" {
    /// A guard: called through `Function.prototype.call`, with the function to
    /// call as `this`.
    type Guard;
    #[wasm_bindgen(method, js_name = call)]
    fn guard0(this: &Guard, function: &JsFunction) -> f64;
    #[wasm_bindgen(method, js_name = call)]
    fn guard1(this: &Guard, function: &JsFunction, a: f64) -> f64;
    #[wasm_bindgen(method, js_name = call)]
    fn guard2(this: &Guard, function: &JsFunction, a: f64, b: f64) -> f64;
    #[wasm_bindgen(method, js_name = call)]
    fn guard3(this: &Guard, function: &JsFunction, a: f64, b: f64, c: f64) -> f64;
    #[wasm_bindgen(method, js_name = call)]
    fn guard4(this: &Guard, function: &JsFunction, a: f64, b: f64, c: f64, d: f64) -> f64;
    #[wasm_bindgen(method, js_name = call)]
    fn guard5(this: &Guard, function: &JsFunction, a: f64, b: f64, c: f64, d: f64, e: f64) -> f64;
    #[wasm_bindgen(method, js_name = call)]
    #[allow(clippy::too_many_arguments)]
    fn guard6(
        this: &Guard,
        function: &JsFunction,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
        f: f64,
    ) -> f64;
    #[wasm_bindgen(method, js_name = call)]
    #[allow(clippy::too_many_arguments)]
    fn guard7(
        this: &Guard,
        function: &JsFunction,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
        f: f64,
        g: f64,
    ) -> f64;
    #[wasm_bindgen(method, js_name = call)]
    #[allow(clippy::too_many_arguments)]
    fn guard8(
        this: &Guard,
        function: &JsFunction,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
        f: f64,
        g: f64,
        h: f64,
    ) -> f64;

    /// Empties the cell of the guards: `undefined`, or an array of the one
    /// value a guard caught.
    type Take;
    #[wasm_bindgen(method, js_name = call)]
    fn take(this: &Take, receiver: &JsValue) -> JsValue;
}

/// The guards of this thread, by whether the function called has a result
/// and by its number of parameters.
pub(crate) struct Guards {
    results: [Guard; MAX_ARITY + 1],
    voids: [Guard; MAX_ARITY + 1],
    take: Take,
}

impl Guards {
    /// The guards, if the host lets them be made.
    fn new() -> Option<Self> {
        let guards = |result: bool| {
            let guards = (0..=MAX_ARITY).map(|arity| {
                let parameters = (0..arity)
                    .map(|index| format!("a{index}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let (call, thrown) = match result {
                    true => (format!("return this({parameters});"), "NaN"),
                    false => (format!("this({parameters}); return 0;"), "1"),
                };
                format!(
                    "function ({parameters}) {{ try {{ {call} }} \
                     catch (error) {{ caught = [error]; return {thrown}; }} }}"
                )
            });
            guards.collect::<Vec<_>>().join(", ")
        };
        let body = format!(
            "\"use strict\"; let caught; return [[{}], [{}], \
             () => {{ const taken = caught; caught = undefined; return taken; }}];",
            guards(true),
            guards(false),
        );
        let make: JsFunction = super::EvaluatedFunction::new("", &body)
            .ok()?
            .unchecked_into();
        let made: js_sys::Array = make.call0(&JsValue::UNDEFINED).ok()?.unchecked_into();
        let guards = |index: u32| -> [Guard; MAX_ARITY + 1] {
            let guards: js_sys::Array = made.get(index).unchecked_into();
            std::array::from_fn(|arity| guards.get(arity as u32).unchecked_into())
        };
        Some(Self {
            results: guards(0),
            voids: guards(1),
            take: made.get(2).unchecked_into(),
        })
    }

    /// The guard of a function of `arity` parameters, with a result or not.
    #[inline]
    fn guard(&self, result: bool, arity: usize) -> &Guard {
        match result {
            true => &self.results[arity],
            false => &self.voids[arity],
        }
    }

    /// What a function returned through its guard, or what it threw.
    #[inline]
    fn returned(&self, result: bool, number: f64) -> Result<f64, JsValue> {
        let threw = match result {
            true => number.is_nan(),
            false => number != 0.0,
        };
        match threw {
            true => self.caught(number),
            false => Ok(number),
        }
    }

    #[cold]
    fn caught(&self, number: f64) -> Result<f64, JsValue> {
        let caught = self.take.take(&JsValue::UNDEFINED);
        match caught.is_undefined() {
            true => Ok(number),
            false => Err(caught.unchecked_into::<js_sys::Array>().get(0)),
        }
    }
}

thread_local! {
    static GUARDS: Option<Guards> = Guards::new();
}

/// The arguments of a call on the numeric lane whose number of parameters is
/// known where it is made: a call of a [`crate::TypedFunction`]. It calls the
/// glue of that many parameters directly, without going through
/// [`Numbers`].
pub(crate) trait Arguments: Sized {
    /// Calls `function` with these numbers, and returns the number it
    /// returns. `result` is whether `function` has a result.
    #[inline]
    fn call(self, function: &JsFunction, result: bool) -> Result<f64, JsValue> {
        GUARDS.with(|guards| self.call_through(guards.as_ref(), function, result))
    }

    /// [`Arguments::call`] through `guards`, or through the `catch` imports
    /// without them.
    fn call_through(
        self,
        guards: Option<&Guards>,
        function: &JsFunction,
        result: bool,
    ) -> Result<f64, JsValue>;
}

macro_rules! arguments {
    ($( $arity:literal => $call:ident, $guard:ident ( $( $argument:ident ),* ); )*) => {
        $(
            impl Arguments for [f64; $arity] {
                #[inline]
                fn call_through(
                    self,
                    guards: Option<&Guards>,
                    function: &JsFunction,
                    result: bool,
                ) -> Result<f64, JsValue> {
                    let [$( $argument ),*] = self;
                    match guards {
                        Some(guards) => {
                            let guard = guards.guard(result, $arity);
                            guards.returned(result, guard.$guard(function, $( $argument ),*))
                        }
                        None => {
                            let function: &NumericFunction = function.unchecked_ref();
                            function.$call(&JsValue::UNDEFINED, $( $argument ),*)
                        }
                    }
                }
            }
        )*
    };
}

arguments! {
    0 => call0, guard0();
    1 => call1, guard1(a);
    2 => call2, guard2(a, b);
    3 => call3, guard3(a, b, c);
    4 => call4, guard4(a, b, c, d);
    5 => call5, guard5(a, b, c, d, e);
    6 => call6, guard6(a, b, c, d, e, f);
    7 => call7, guard7(a, b, c, d, e, f, g);
    8 => call8, guard8(a, b, c, d, e, f, g, h);
}

/// Calls `function` with `numbers`, and returns the number it returns.
/// `result` is whether `function` has a result.
pub(crate) fn call(
    function: &JsFunction,
    (numbers, arity): &Numbers,
    result: bool,
) -> Result<f64, JsValue> {
    match numbers[..*arity] {
        [] => [].call(function, result),
        [a] => [a].call(function, result),
        [a, b] => [a, b].call(function, result),
        [a, b, c] => [a, b, c].call(function, result),
        [a, b, c, d] => [a, b, c, d].call(function, result),
        [a, b, c, d, e] => [a, b, c, d, e].call(function, result),
        [a, b, c, d, e, f] => [a, b, c, d, e, f].call(function, result),
        [a, b, c, d, e, f, g] => [a, b, c, d, e, f, g].call(function, result),
        [a, b, c, d, e, f, g, h] => [a, b, c, d, e, f, g, h].call(function, result),
        _ => unreachable!("the numeric lane ends at {MAX_ARITY} parameters"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::wasm_bindgen_test;

    /// What a call returned, comparable: the bits of a number (any `NaN` the
    /// same), or the value thrown.
    #[derive(Debug)]
    enum Outcome {
        Returned(Option<u64>),
        Threw(JsValue),
    }

    impl PartialEq for Outcome {
        fn eq(&self, other: &Self) -> bool {
            match (self, other) {
                (Self::Returned(a), Self::Returned(b)) => a == b,
                (Self::Threw(a), Self::Threw(b)) => js_sys::Object::is(a, b),
                _ => false,
            }
        }
    }

    fn outcome(returned: Result<f64, JsValue>, result: bool) -> Outcome {
        match returned {
            // What a function without a result returns is not used.
            Ok(_) if !result => Outcome::Returned(None),
            Ok(number) if number.is_nan() => Outcome::Returned(Some(u64::MAX)),
            Ok(number) => Outcome::Returned(Some(number.to_bits())),
            Err(thrown) => Outcome::Threw(thrown),
        }
    }

    /// Through a guard, a call returns and throws what it does through the
    /// `catch` glue, whatever the function returns or throws and however
    /// many parameters it has, and a call after one that threw is not taken
    /// for one that threw.
    #[wasm_bindgen_test]
    fn a_guard_returns_and_throws_as_the_catch_glue_does() {
        let guards = Guards::new().expect("this host makes functions from text");
        let thrown = js_sys::Object::new();
        let functions: js_sys::Array = JsFunction::new_with_args(
            "thrown",
            "const wrapped = [thrown];
            return [
                (...numbers) => numbers.reduce((sum, number) => sum * 2 + number, 0.5),
                () => NaN,
                () => 0,
                () => 1,
                () => -0,
                () => undefined,
                () => 'text',
                () => { throw thrown; },
                () => { throw undefined; },
                () => { throw null; },
                () => { throw NaN; },
                () => { throw 0; },
                () => { throw 1; },
                () => { throw wrapped; },
            ];",
        )
        .call1(&JsValue::UNDEFINED, &thrown)
        .unwrap()
        .unchecked_into();
        let returns_nan: JsFunction = functions.get(1).unchecked_into();
        let numbers = [1.5, -2.0, 3.25, 0.0, -0.0, 1e300, -7.0, 8.0];
        for function in functions.iter() {
            let function: JsFunction = function.unchecked_into();
            for arity in 0..=MAX_ARITY {
                let numbers = copy(&numbers[..arity]);
                for result in [true, false] {
                    let guarded = call_through(Some(&guards), &function, &numbers, result);
                    let caught = call_through(None, &function, &numbers, result);
                    let (guarded, caught) = (outcome(guarded, result), outcome(caught, result));
                    assert_eq!(guarded, caught, "{function:?} of {arity}");

                    let numbers = copy(&[]);
                    let next = call_through(Some(&guards), &returns_nan, &numbers, true);
                    assert_eq!(outcome(next, true), Outcome::Returned(Some(u64::MAX)));
                }
            }
        }
    }

    /// A host function returns and throws what a [`closure`] of the same
    /// body does, with the arguments in the same order, and a host function
    /// that returns after one that threw returns.
    #[wasm_bindgen_test]
    fn a_host_function_returns_and_throws_as_a_closure_does() {
        assert!(
            HOSTS.with(Option::is_some),
            "this host makes functions from text"
        );
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
                assert_eq!(call(&made, arity), call(&wrapped, arity), "of {arity}");
                assert_eq!(call(&after, 0), Outcome::Returned(Some(1.0f64.to_bits())));
            }
        }
    }

    fn call_through(
        guards: Option<&Guards>,
        function: &JsFunction,
        (numbers, arity): &Numbers,
        result: bool,
    ) -> Result<f64, JsValue> {
        match numbers[..*arity] {
            [] => [].call_through(guards, function, result),
            [a] => [a].call_through(guards, function, result),
            [a, b] => [a, b].call_through(guards, function, result),
            [a, b, c] => [a, b, c].call_through(guards, function, result),
            [a, b, c, d] => [a, b, c, d].call_through(guards, function, result),
            [a, b, c, d, e] => [a, b, c, d, e].call_through(guards, function, result),
            [a, b, c, d, e, f] => [a, b, c, d, e, f].call_through(guards, function, result),
            [a, b, c, d, e, f, g] => [a, b, c, d, e, f, g].call_through(guards, function, result),
            [a, b, c, d, e, f, g, h] => {
                [a, b, c, d, e, f, g, h].call_through(guards, function, result)
            }
            _ => unreachable!(),
        }
    }
}
