//! This test suite does all the tests that involve any compiler
//! implementation, such as: singlepass, cranelift or llvm depending
//! on what's available on the target.

mod functions;
mod imports;
mod multi_value_imports;
mod traps;
mod utils;
