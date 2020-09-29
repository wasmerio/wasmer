//! entrypoints for the standard C API

#[macro_use]
pub mod macros;

pub mod engine;
pub mod externals;
pub mod instance;
pub mod module;
pub mod store;
pub mod trap;
pub mod types;
pub mod value;

#[cfg(feature = "wasi")]
pub mod wasi;
