#![doc(html_favicon_url = "https://wasmer.io/static/icons/favicon.ico")]
#![doc(html_logo_url = "https://avatars3.githubusercontent.com/u/44205449?s=200&v=4")]

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
#![deny(
    dead_code,
    unused_imports,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
extern crate wasmer_runtime;
extern crate wasmer_runtime_core;

pub mod error;
pub mod export;
pub mod global;
pub mod import;
pub mod instance;
pub mod memory;
pub mod module;
pub mod table;
// `not(target_family = "windows")` is simpler than `unix`.  See build.rs
// if you want to change the meaning of these `cfg`s in the header file.
#[cfg(all(not(target_family = "windows"), target_arch = "x86_64"))]
pub mod trampoline;
pub mod value;

#[allow(non_camel_case_types)]
#[repr(C)]
pub enum wasmer_result_t {
    WASMER_OK = 1,
    WASMER_ERROR = 2,
}

#[repr(C)]
pub struct wasmer_limits_t {
    pub min: u32,
    pub max: wasmer_limit_option_t,
}

#[repr(C)]
pub struct wasmer_limit_option_t {
    pub has_some: bool,
    pub some: u32,
}

#[repr(C)]
pub struct wasmer_byte_array {
    pub bytes: *const u8,
    pub bytes_len: u32,
}

impl wasmer_byte_array {
    /// Get the data as a slice
    pub unsafe fn as_slice<'a>(&self) -> &'a [u8] {
        get_slice_checked(self.bytes, self.bytes_len as usize)
    }

    /// Copy the data into an owned Vec
    pub unsafe fn as_vec(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.bytes_len as usize);
        out.extend_from_slice(self.as_slice());

        out
    }

    /// Read the data as a &str, returns an error if the string is not valid UTF8
    pub unsafe fn as_str<'a>(&self) -> Result<&'a str, std::str::Utf8Error> {
        std::str::from_utf8(self.as_slice())
    }
}

/// Gets a slice from a pointer and a length, returning an empty slice if the
/// pointer is null
#[inline]
pub(crate) unsafe fn get_slice_checked<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
    if ptr.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(ptr, len)
    }
}
