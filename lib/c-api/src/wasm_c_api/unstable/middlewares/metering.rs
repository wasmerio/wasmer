//! Unstable non-standard Wasmer-specific API that contains everything
//! to create the middleware metering API.
//!
//! The metering middleware is used for tracking how many operators
//! are executed in total and putting a limit on the total number of
//! operators executed.
//!
//! # Example
//!
//! ```rust
//! # use wasmer_inline_c::assert_c;
//! # fn main() {
//! #    (assert_c! {
//! # #include "tests/wasmer.h"
//! #
//! // Define our “cost function”.
//! uint64_t cost_function(wasmer_parser_operator_t wasm_operator) {
//!     switch(wasm_operator) {
//!         // `local.get` and `i32.const` cost 1 unit.
//!         case LocalGet:
//!         case I32Const:
//!             return 1;
//!
//!         // `i32.add` costs 2 units.
//!         case I32Add:
//!             return 2;
//!
//!         // The other operations are free.
//!         default:
//!             return 0;
//!     }
//! }
//!
//! int main() {
//!     // Create a new metering middleware, with our cost function.
//!     wasmer_metering_t* metering = wasmer_metering_new(10, cost_function);
//!
//!     // Consume `metering` to produce a generic `wasmer_middleware_t` value.
//!     wasmer_middleware_t* middleware = wasmer_metering_as_middleware(metering);
//!     
//!     // Create a new configuration, and push the middleware in it.
//!     wasm_config_t* config = wasm_config_new();
//!     wasm_config_push_middleware(config, middleware);
//!     
//!     // Create the engine and the store based on the configuration.
//!     wasm_engine_t* engine = wasm_engine_new_with_config(config);
//!     wasm_store_t* store = wasm_store_new(engine);
//!     
//!     // Create the new WebAssembly module.
//!     wasm_byte_vec_t wat;
//!     wasmer_byte_vec_new_from_string(
//!         &wat,
//!         "(module\n"
//!         "  (type $add_t (func (param i32) (result i32)))\n"
//!         "  (func $add_two_f (type $add_t) (param $value i32) (result i32)\n"
//!         "    local.get $value\n"
//!         "    i32.const 1\n"
//!         "    i32.add)\n"
//!         "  (export \"add_two\" (func $add_two_f)))"
//!     );
//!     wasm_byte_vec_t wasm;
//!     wat2wasm(&wat, &wasm);
//!
//!     wasm_module_t* module = wasm_module_new(store, &wasm);
//!     assert(module);
//!     
//!     // Instantiate the module.
//!     wasm_extern_vec_t imports = WASM_EMPTY_VEC;
//!     wasm_trap_t* trap = NULL;
//!     wasm_instance_t* instance = wasm_instance_new(store, module, &imports, &trap);
//!     assert(instance);
//!     
//!     // Here we go. At this step, we will get the `add_two` exported function, and
//!     // call it.
//!     wasm_extern_vec_t exports;
//!     wasm_instance_exports(instance, &exports);
//!     assert(exports.size >= 1);
//!     assert(wasm_extern_kind(exports.data[0]) == WASM_EXTERN_FUNC);
//!
//!     const wasm_func_t* add_two = wasm_extern_as_func(exports.data[0]);
//!     assert(add_two);
//!
//!     wasm_val_t arguments[1] = { WASM_I32_VAL(41) };
//!     wasm_val_t results[1] = { WASM_INIT_VAL };
//!
//!     wasm_val_vec_t arguments_as_array = WASM_ARRAY_VEC(arguments);
//!     wasm_val_vec_t results_as_array = WASM_ARRAY_VEC(results);
//!
//!     // Let's call `add_two` for the first time!
//!     {
//!         trap = wasm_func_call(add_two, &arguments_as_array, &results_as_array);
//!         assert(trap == NULL);
//!         assert(results[0].of.i32 == 42);
//!
//!         // There is 6 points left!
//!         assert(wasmer_metering_get_remaining_points(instance) == 6);
//!         assert(wasmer_metering_points_are_exhausted(instance) == false);
//!     }
//!
//!     // Let's call `add_two` for the second time!
//!     {
//!         trap = wasm_func_call(add_two, &arguments_as_array, &results_as_array);
//!         assert(trap == NULL);
//!         assert(results[0].of.i32 == 42);
//!
//!         // There is 2 points left!
//!         assert(wasmer_metering_get_remaining_points(instance) == 2);
//!         assert(wasmer_metering_points_are_exhausted(instance) == false);
//!     }
//!
//!     // Let's call `add_two` for the third time!
//!     {
//!         trap = wasm_func_call(add_two, &arguments_as_array, &results_as_array);
//!         // Oh, it failed!
//!         assert(trap != NULL);
//!
//!         // There is 0 point left… they are exhausted.
//!         assert(wasmer_metering_points_are_exhausted(instance) == true);
//!     }
//!
//!     wasm_extern_vec_delete(&exports);
//!     wasm_instance_delete(instance);
//!     wasm_module_delete(module);
//!     wasm_store_delete(store);
//!     wasm_engine_delete(engine);
//!
//!     return 0;
//! }
//! #    })
//! #    .success();
//! # }
//! ```

use super::super::super::instance::wasm_instance_t;
use super::super::parser::operator::wasmer_parser_operator_t;
use super::wasmer_middleware_t;
use std::sync::Arc;
use wasmer_api::wasmparser::Operator;
use wasmer_middlewares::{
    metering::{get_remaining_points, set_remaining_points, MeteringPoints},
    Metering,
};

/// Opaque type representing a metering middleware.
///
/// To transform this specific middleware into a generic one, please
/// see [`wasmer_metering_as_middleware`].
///
/// # Example
///
/// See module's documentation.
#[allow(non_camel_case_types, clippy::type_complexity)]
pub struct wasmer_metering_t {
    pub(crate) inner: Arc<Metering<Box<dyn Fn(&Operator) -> u64 + Send + Sync>>>,
}

/// Function type to represent a user-defined cost function
/// implemented in C.
///
/// # Example
///
/// See module's documentation.
#[allow(non_camel_case_types)]
pub type wasmer_metering_cost_function_t =
    extern "C" fn(wasm_operator: wasmer_parser_operator_t) -> u64;

/// Creates a new metering middleware with an initial limit, i.e. a
/// total number of operators to execute (regarding their respective
/// cost), in addition to a cost function. The cost function defines
/// the cost of an operation, that will decrease the initial limit.
///
/// # Example
///
/// See module's documentation.
#[no_mangle]
pub extern "C" fn wasmer_metering_new(
    initial_limit: u64,
    cost_function: wasmer_metering_cost_function_t,
) -> Box<wasmer_metering_t> {
    let cost_function = move |operator: &Operator| -> u64 { cost_function(operator.into()) };

    Box::new(wasmer_metering_t {
        inner: Arc::new(Metering::new(initial_limit, Box::new(cost_function))),
    })
}

/// Deletes a [`wasmer_metering_t`].
///
/// # Example
///
/// See module's documentation.
#[no_mangle]
pub extern "C" fn wasmer_metering_delete(_metering: Option<Box<wasmer_metering_t>>) {}

/// Returns the remaining metering points. `u64::MAX` means
/// points are exhausted, otherwise it returns the number of
/// points. Notice that it could include zero! Zero doesn't mean
/// points are exhausted _yet_.
///
/// # Example
///
/// See module's documentation.
#[no_mangle]
pub unsafe extern "C" fn wasmer_metering_get_remaining_points(
    instance: &mut wasm_instance_t,
) -> u64 {
    match get_remaining_points(&mut instance.store.store_mut(), &instance.inner) {
        MeteringPoints::Remaining(value) => value,
        MeteringPoints::Exhausted => std::u64::MAX,
    }
}

/// Returns true if the remaning points are exhausted, false otherwise.
///
/// # Example
///
/// See module's documentation.
#[no_mangle]
pub unsafe extern "C" fn wasmer_metering_points_are_exhausted(
    instance: &mut wasm_instance_t,
) -> bool {
    matches!(
        get_remaining_points(&mut instance.store.store_mut(), &instance.inner),
        MeteringPoints::Exhausted,
    )
}

/// Set a new amount of points for the given metering middleware.
///
/// # Example
///
/// This example only illustrates the
/// `wasmer_metering_set_remaining_points` function, as the number of
/// points aren't updated by the WebAssembly module execution
///
/// ```rust
/// # use wasmer_inline_c::assert_c;
/// # fn main() {
/// #    (assert_c! {
/// # #include "tests/wasmer.h"
/// #
/// // Define a dummy “cost function”.
/// uint64_t cost_function(wasmer_parser_operator_t wasm_operator) {
///     switch(wasm_operator) {
///         default:
///             return 0;
///     }
/// }
///
/// int main() {
///     // Set the initial amount of points to 7.
///     wasmer_metering_t* metering = wasmer_metering_new(7, cost_function);
///
///     // Consume `metering` to produce `middleware`.
///     wasmer_middleware_t* middleware = wasmer_metering_as_middleware(metering);
///
///     // Create the configuration (which consumes `middleware`),
///     // the engine, and the store.
///     wasm_config_t* config = wasm_config_new();
///     wasm_config_push_middleware(config, middleware);
///     
///     wasm_engine_t* engine = wasm_engine_new_with_config(config);
///
///     wasm_store_t* store = wasm_store_new(engine);
///     
///     // Create the module and instantiate it.
///     wasm_byte_vec_t wat;
///     wasmer_byte_vec_new_from_string(&wat, "(module)");
///     wasm_byte_vec_t wasm;
///     wat2wasm(&wat, &wasm);
///
///     wasm_module_t* module = wasm_module_new(store, &wasm);
///     assert(module);
///     
///     wasm_extern_vec_t imports = WASM_EMPTY_VEC;
///     wasm_trap_t* trap = NULL;
///     wasm_instance_t* instance = wasm_instance_new(store, module, &imports, &trap);
///     assert(instance);
///
///     // Read the number of points.
///     assert(wasmer_metering_get_remaining_points(instance) == 7);
///
///     // Set a new number of points.
///     wasmer_metering_set_remaining_points(instance, 42);
///
///     // Read the number of points.
///     assert(wasmer_metering_get_remaining_points(instance) == 42);
///
///     wasm_instance_delete(instance);
///     wasm_module_delete(module);
///     wasm_store_delete(store);
///     wasm_engine_delete(engine);
///
///     return 0;
/// }
/// #    })
/// #    .success();
/// # }
/// ```
#[no_mangle]
pub unsafe extern "C" fn wasmer_metering_set_remaining_points(
    instance: &mut wasm_instance_t,
    new_limit: u64,
) {
    set_remaining_points(&mut instance.store.store_mut(), &instance.inner, new_limit);
}

/// Transforms a [`wasmer_metering_t`] into a generic
/// [`wasmer_middleware_t`], to then be pushed in the configuration with
/// [`wasm_config_push_middleware`][super::wasm_config_push_middleware].
///
/// This function takes ownership of `metering`.
///
/// # Example
///
/// See module's documentation.
#[no_mangle]
pub extern "C" fn wasmer_metering_as_middleware(
    metering: Option<Box<wasmer_metering_t>>,
) -> Option<Box<wasmer_middleware_t>> {
    let metering = metering?;

    Some(Box::new(wasmer_middleware_t {
        inner: metering.inner,
    }))
}
