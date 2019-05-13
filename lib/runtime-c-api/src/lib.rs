//! # Wasmer Runtime C API
//!
//! Wasmer is a standalone JIT WebAssembly runtime, aiming to be fully
//! compatible with Emscripten, Rust and Go. [Learn
//! more](https://github.com/wasmerio/wasmer).
//!
//! This crate exposes a C and C++ API for the Wasmer runtime.
//!
//! # Usage
//!
//! The C and C++ header files can be found in the source tree of this
//! crate, respectively [`wasmer.h`][wasmer_h] and
//! [`wasmer.hh`][wasmer_hh]. They are automatically generated, and always
//! up-to-date in this repository.
//!
//! Here is a simple example to use the C API:
//!
//! ```c
//! #include <stdio.h>
//! #include "wasmer.h"
//! #include <assert.h>
//! #include <stdint.h>
//!
//! int main()
//! {
//!     // Read the Wasm file bytes.
//!     FILE *file = fopen("sum.wasm", "r");
//!     fseek(file, 0, SEEK_END);
//!     long len = ftell(file);
//!     uint8_t *bytes = malloc(len);
//!     fseek(file, 0, SEEK_SET);
//!     fread(bytes, 1, len, file);
//!     fclose(file);
//!
//!     // Prepare the imports.
//!     wasmer_import_t imports[] = {};
//!
//!     // Instantiate!
//!     wasmer_instance_t *instance = NULL;
//!     wasmer_result_t instantiation_result = wasmer_instantiate(&instance, bytes, len, imports, 0);
//!
//!     assert(instantiation_result == WASMER_OK);
//!
//!     // Let's call a function.
//!     // Start by preparing the arguments.
//!
//!     // Value of argument #1 is `7i32`.
//!     wasmer_value_t argument_one;
//!     argument_one.tag = WASM_I32;
//!     argument_one.value.I32 = 7;
//!
//!     // Value of argument #2 is `8i32`.
//!     wasmer_value_t argument_two;
//!     argument_two.tag = WASM_I32;
//!     argument_two.value.I32 = 8;
//!
//!     // Prepare the arguments.
//!     wasmer_value_t arguments[] = {argument_one, argument_two};
//!
//!     // Prepare the return value.
//!     wasmer_value_t result_one;
//!     wasmer_value_t results[] = {result_one};
//!
//!     // Call the `sum` function with the prepared arguments and the return value.
//!     wasmer_result_t call_result = wasmer_instance_call(instance, "sum", arguments, 2, results, 1);
//!
//!     // Let's display the result.
//!     printf("Call result:  %d\n", call_result);
//!     printf("Result: %d\n", results[0].value.I32);
//!
//!     // `sum(7, 8) == 15`.
//!     assert(results[0].value.I32 == 15);
//!     assert(call_result == WASMER_OK);
//!
//!     wasmer_instance_destroy(instance);
//!
//!     return 0;
//! }
//! ```
//!
//! [wasmer_h]: ./wasmer.h
//! [wasmer_hh]: ./wasmer.hh
#![deny(unused_imports, unused_variables, unused_unsafe, unreachable_patterns)]

extern crate wasmer_runtime;
extern crate wasmer_runtime_core;

use libc::{uint32_t, uint8_t};

pub mod error;
pub mod export;
pub mod global;
pub mod import;
pub mod instance;
pub mod memory;
pub mod module;
pub mod table;
pub mod value;

#[allow(non_camel_case_types)]
#[repr(C)]
pub enum wasmer_result_t {
    WASMER_OK = 1,
    WASMER_ERROR = 2,
}

#[repr(C)]
pub struct wasmer_limits_t {
    pub min: uint32_t,
    pub max: wasmer_limit_option_t,
}

#[repr(C)]
pub struct wasmer_limit_option_t {
    pub has_some: bool,
    pub some: uint32_t,
}

#[repr(C)]
pub struct wasmer_byte_array {
    bytes: *const uint8_t,
    bytes_len: uint32_t,
}
