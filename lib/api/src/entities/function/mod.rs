//! Defines the [`Function`] and [`HostFunction`] types, the [`FunctionLike`] trait for implementors and useful
//! traits and data types to interact with them.

pub(crate) mod wasm_function;
pub use wasm_function::*;

pub(crate) mod host_function;
pub use host_function::*;

pub(crate) mod env;
pub use env::*;
