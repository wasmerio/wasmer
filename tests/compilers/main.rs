//! This test suite does all the tests that involve any compiler
//! implementation, such as: singlepass, cranelift or llvm depending
//! on what's available on the target.

#[macro_use]
extern crate compiler_test_derive;

mod config;
mod deterministic;
mod imports;
mod issues;
mod metering;
mod middlewares;
// mod multi_value_imports;
mod artifact;
mod serialize;
mod traps;
mod typed_functions;
mod wasi;
mod wast;

pub use crate::config::{Compiler, Config};
pub use crate::wasi::run_wasi;
pub use crate::wast::run_wast;
pub use wasmer_wast::WasiFileSystemKind;
