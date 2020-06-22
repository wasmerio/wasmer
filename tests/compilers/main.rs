#![cfg(feature = "compiler")]

//! This test suite does all the tests that involve any compiler
//! implementation, such as: singlepass, cranelift or llvm depending
//! on what's available on the target.

mod imports;
mod metering;
mod middlewares;
mod multi_value_imports;
mod native_functions;
mod serialize;
mod traps;
mod utils;
mod wasi;
mod wast;

pub use crate::utils::get_compiler;
pub use crate::wasi::run_wasi;
pub use crate::wast::run_wast;
