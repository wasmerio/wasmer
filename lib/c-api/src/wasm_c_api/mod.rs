//! Implementation of the [official WebAssembly C
//! API](https://github.com/WebAssembly/wasm-c-api) for Wasmer.
//!
//! We would like to remind the reader that this official standard can
//! be characterized as a _living standard_. As such, the API is not
//! yet stable, even though it shows maturity over time. The API is
//! described by the `wasm.h` C header, which is included by
//! `wasmer.h` C header file (which contains extension of the
//! standard API, for example to provide WASI or vendor-specific
//! features).
//!
//! # Quick Guide
//!
//! Usually, the user first needs to create an [`engine`] and a
//! [`store`]. Once it's done, the user needs to create a [`module`]
//! and then [instantiate][instance] it. When instantiating the
//! module, the user is able to pass a set of
//! [imports][externals]. With an instance, the user is able to call
//! the [exports][instance::wasm_instance_exports].
//!
//! Every module comes with examples and entry points to guide the
//! discovery of this API.

/// `Context`.
mod function_env;

/// Private Rust macros.
#[macro_use]
mod macros;

/// An engine drives the compilation and the runtime.
///
/// Entry points: A default engine is created with
/// [`wasm_engine_new`][engine::wasm_engine_new] and freed with
/// [`wasm_engine_delete`][engine::wasm_engine_delete].
///
/// # Example
///
/// The simplest way to get a default engine is the following:
///
/// ```rust
/// # use wasmer_inline_c::assert_c;
/// # fn main() {
/// #    (assert_c! {
/// # #include "tests/wasmer.h"
/// #
/// int main() {
///     // Create the engine.
///     wasm_engine_t* engine = wasm_engine_new();
///
///     // Check we have a valid engine!
///     assert(engine);
///
///     // Free everything.
///     wasm_engine_delete(engine);
///
///     return 0;
/// }
/// #    })
/// #    .success();
/// # }
/// ```
///
/// To configure the engine, see the [`wasm_config_new`][engine::wasm_config_new].
pub mod engine;

/// cbindgen:ignore
pub mod externals;

/// A WebAssembly instance is a stateful, executable instance of a
/// WebAssembly module.
///
/// Instance objects contain all the exported WebAssembly functions,
/// memories, tables and globals that allow interacting with
/// WebAssembly.
///
/// Entry points: A WebAssembly instance is created with
/// [`wasm_instance_new`][instance::wasm_instance_new] and freed with
/// [`wasm_instance_delete`][instance::wasm_instance_delete].
///
/// # Example
///
/// The simplest way to instantiate a Wasm module is the following:
///
/// ```rust
/// # use wasmer_inline_c::assert_c;
/// # fn main() {
/// #    (assert_c! {
/// # #include "tests/wasmer.h"
/// #
/// int main() {
///     // Create the engine and the store.
///     wasm_engine_t* engine = wasm_engine_new();
///     wasm_store_t* store = wasm_store_new(engine);
///
///     // Create a WebAssembly module from a WAT definition.
///     wasm_byte_vec_t wat;
///     wasmer_byte_vec_new_from_string(&wat, "(module)");
///     wasm_byte_vec_t wasm;
///     wat2wasm(&wat, &wasm);
///
///     // Create the module.
///     wasm_module_t* module = wasm_module_new(store, &wasm);
///     assert(module);
///
///     // Instantiate the module.
///     wasm_extern_vec_t imports = WASM_EMPTY_VEC;
///     wasm_trap_t* trap = NULL;
///
///     wasm_instance_t* instance = wasm_instance_new(store, module, &imports, &trap);
///     assert(instance);
///
///     // Now do something with the instance, like calling the
///     // exports with `wasm_instance_exports`.
///
///     // Free everything.
///     wasm_instance_delete(instance);
///     wasm_module_delete(module);
///     wasm_byte_vec_delete(&wasm);
///     wasm_byte_vec_delete(&wat);
///     wasm_store_delete(store);
///     wasm_engine_delete(engine);
///
///     return 0;
/// }
/// #    })
/// #    .success();
/// # }
/// ```
///
/// cbindgen:ignore
pub mod instance;

/// A WebAssembly module contains stateless WebAssembly code that has
/// already been compiled and can be instantiated multiple times.
///
/// Entry points: A WebAssembly module is created with
/// [`wasm_module_new`][module::wasm_module_new] and freed with
/// [`wasm_module_delete`][module::wasm_module_delete].
///
/// # Example
///
/// ```rust
/// # use wasmer_inline_c::assert_c;
/// # fn main() {
/// #    (assert_c! {
/// # #include "tests/wasmer.h"
/// #
/// int main() {
///     // Create the engine and the store.
///     wasm_engine_t* engine = wasm_engine_new();
///     wasm_store_t* store = wasm_store_new(engine);
///
///     // Create a WebAssembly module from a WAT definition.
///     wasm_byte_vec_t wat;
///     wasmer_byte_vec_new_from_string(&wat, "(module)");
///     wasm_byte_vec_t wasm;
///     wat2wasm(&wat, &wasm);
///    
///     // Create the module.
///     wasm_module_t* module = wasm_module_new(store, &wasm);
///
///     // It works!
///     assert(module);
///    
///     // Free everything.
///     wasm_byte_vec_delete(&wasm);
///     wasm_byte_vec_delete(&wat);
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
///
/// cbindgen:ignore
pub mod module;

/// A store represents all global state that can be manipulated by
/// WebAssembly programs. It consists of the runtime representation of
/// all instances of functions, tables, memories, and globals that
/// have been allocated during the lifetime of the abstract machine.
///
/// The store holds the [engine] (that is —amonst many things— used to
/// compile the Wasm bytes into a valid [module] artifact), in addition
/// to extra private types.
///
/// Entry points: A store is created with
/// [`wasm_store_new`][store::wasm_store_new] and freed with
/// [`wasm_store_delete`][store::wasm_store_delete]. To customize the
/// engine the store holds, see
/// [`wasm_config_new`][engine::wasm_config_new].
///
/// # Example
///
/// ```rust
/// # use wasmer_inline_c::assert_c;
/// # fn main() {
/// #    (assert_c! {
/// # #include "tests/wasmer.h"
/// #
/// int main() {
///     // Create the engine.
///     wasm_engine_t* engine = wasm_engine_new();
///
///     // Create the store.
///     wasm_store_t* store = wasm_store_new(engine);
///
///     // It works!
///     assert(store);
///    
///     // Free everything.
///     wasm_store_delete(store);
///     wasm_engine_delete(engine);
///
///     return 0;
/// }
/// #    })
/// #    .success();
/// # }
/// ```
///
/// cbindgen:ignore
pub mod store;

/// A trap represents an error which stores trace message with
/// backtrace.
///
/// # Example
///
/// ```rust
/// # use wasmer_inline_c::assert_c;
/// # fn main() {
/// #    (assert_c! {
/// # #include "tests/wasmer.h"
/// #
/// int main() {
///     // Create an engine and a store.
///     wasm_engine_t* engine = wasm_engine_new();
///     wasm_store_t* store = wasm_store_new(engine);
///
///     // Create the trap message.
///     wasm_message_t message;
///     wasm_name_new_from_string_nt(&message, "foobar");
///
///     // Create the trap with its message.
///     // The backtrace will be generated automatically.
///     wasm_trap_t* trap = wasm_trap_new(store, &message);
///     assert(trap);
///
///     wasm_name_delete(&message);
///
///     // Do something with the trap.
///
///     // Free everything.
///     wasm_trap_delete(trap);
///     wasm_store_delete(store);
///     wasm_engine_delete(engine);
///
///     return 0;
/// }
/// #    })
/// #    .success();
/// # }
/// ```
///
/// Usually, a trap is returned from a host function (an imported
/// function).
///
/// cbindgen:ignore
pub mod trap;

/// cbindgen:ignore
pub mod types;

/// This module contains _unstable non-standard_ C API.
///
/// Use them at your own risks. The API is subject to change or to
/// break without any plan to keep any compatibility :-).
pub mod unstable;

/// Possible runtime values that a WebAssembly module can either
/// consume or produce.
///
/// cbindgen:ignore
pub mod value;

/// Wasmer-specific API to get or query the version of this Wasm C API.
///
/// The `wasmer.h` file provides the `WASMER_VERSION`,
/// `WASMER_VERSION_MAJOR`, `WASMER_VERSION_MINOR`,
/// `WASMER_VERSION_PATCH` and `WASMER_VERSION_PRE`
/// constants. However, in absence of this header file, it is possible
/// to retrieve the same information with their respective functions,
/// namely [`wasmer_version`][version::wasmer_version],
/// [`wasmer_version_major`][version::wasmer_version_major],
/// [`wasmer_version_minor`][version::wasmer_version_minor],
/// [`wasmer_version_patch`][version::wasmer_version_patch], and
/// [`wasmer_version_pre`][version::wasmer_version_pre].
///
/// # Example
///
/// ```rust
/// # use wasmer_inline_c::assert_c;
/// # fn main() {
/// #    (assert_c! {
/// # #include "tests/wasmer.h"
/// #
/// int main() {
///     // Get and print the version.
///     const char* version = wasmer_version();
///     printf("%s", version);
///
///     // No need to free the string. It's statically allocated on
///     // the Rust side.
///
///     return 0;
/// }
/// #    })
/// #    .success()
/// #    .stdout(env!("CARGO_PKG_VERSION"));
/// # }
/// ```
pub mod version;

#[cfg(feature = "wasi")]
pub mod wasi;

/// Wasmer-specific API to transform the WAT format into Wasm bytes.
///
/// It is used mostly for testing or for small program purposes.
///
/// # Example
///
/// ```rust
/// # use wasmer_inline_c::assert_c;
/// # fn main() {
/// #    (assert_c! {
/// # #include "tests/wasmer.h"
/// #
/// int main() {
///     // Our WAT module.
///     wasm_byte_vec_t wat;
///     wasm_byte_vec_new(&wat, 8, "(module)");
///
///     // Our Wasm bytes.
///     wasm_byte_vec_t wasm;
///     wat2wasm(&wat, &wasm);
///
///     // It works!
///     assert(wasm.size > 0);
///
///     // Free everything.
///     wasm_byte_vec_delete(&wasm);
///     wasm_byte_vec_delete(&wat);
///
///     return 0;
/// }
/// #    })
/// #    .success();
/// # }
/// ```
#[cfg(feature = "wat")]
pub mod wat;
