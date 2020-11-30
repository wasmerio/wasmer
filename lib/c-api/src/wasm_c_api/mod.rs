//! Entrypoints for the standard C API

#[macro_use]
pub mod macros;

/// The engine drives the compilation and the runtime.
///
/// Entry points: A default engine is created with `wasm_engine_new`
/// and freed with `wasm_engine_delete`.
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
/// `wasm_instance_new` and freed with `wasm_instance_delete`.
///
/// cbindgen:ignore
pub mod instance;

/// A WebAssembly module contains stateless WebAssembly code that has
/// already been compiled and can be instantiated multiple times.
///
/// Entry points: A WebAssembly module is created with
/// `wasm_module_new` and freed with `wasm_module_delete`.
///
/// cbindgen:ignore
pub mod module;

/// cbindgen:ignore
pub mod store;

/// cbindgen:ignore
pub mod trap;

/// cbindgen:ignore
pub mod types;

/// cbindgen:ignore
pub mod value;

#[cfg(feature = "wasi")]
pub mod wasi;

pub mod wasmer;

#[cfg(feature = "wat")]
pub mod wat;
