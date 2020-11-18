//! Wasmer C API.
//!
//! [Wasmer](https://github.com/wasmerio/wasmer) is the leading
//! WebAssembly runtime. This crate exposes the C and C++ bindings.
//!
//! This crate provides 2 API:
//!
//! * `deprecated`, which is the old one, and is represented by the
//!   `wasmer.h` and the `wasmer.hh` C header files,
//! * `wasm_c_api`, which is the new, standard C API, and is
//!   represented by the `wasmer_wasm.h` C header file.
//!
//! The `wasm_c_api` follows the [official WebAssembly C and C++
//! API](https://github.com/WebAssembly/wasm-c-api). It provides
//! however some extensions, like the `wasi_*` types and functions,
//! which aren't yet defined by the standard. The `wasmer_wasm.h`
//! header file already depends on the `wasm.h` file. A copy lands in
//! this repository for the sake of simplicity.

#![doc(html_favicon_url = "https://wasmer.io/static/icons/favicon.ico")]
#![doc(html_logo_url = "https://github.com/wasmerio.png?size=200")]
// temporary while in transition
#![allow(unused_variables)]
#![deny(
    dead_code,
    unused_imports,
    // temporarily disabled
    //unused_variables,
    unused_unsafe,
    unreachable_patterns
)]

#[cfg(feature = "deprecated")]
pub mod deprecated;
pub mod error;
mod ordered_resolver;
pub mod wasm_c_api;
