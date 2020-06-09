//! This test suite does all the tests that involve any compiler
//! implementation, such as: singlepass, cranelift or llvm depending
//! on what's available on the target.

#[macro_use]
mod macros;
mod imports;
mod multi_value_imports;
mod wast;
#[macro_use]
extern crate lazy_static;
