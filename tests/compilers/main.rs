//! This test suite does all the tests that involve any compiler
//! implementation, such as: singlepass, cranelift or llvm depending
//! on what's available on the target.

#[macro_use]
extern crate compiler_test_derive;

mod artifact;
mod config;
mod deterministic;
mod imports;
mod issues;
#[cfg(feature = "middlewares")]
mod metering;
mod middlewares;
mod multi_value_imports;
mod progress;
mod serialize;
mod traps;
mod typed_functions;
#[cfg(feature = "wast")]
mod wast;

pub use crate::config::{Compiler, Config};
#[cfg(feature = "wast")]
pub use crate::wast::run_wast;
