//! Wasmer C API.
//!
//! [Wasmer](https://github.com/wasmerio/wasmer) is the leading
//! WebAssembly runtime. Wasmer is written in Rust; this crate
//! exposes its C and C++ bindings.
//!
//! This crate provides 2 different API:
//!
//! 1. [`deprecated`], which is the old one, and is represented by the
//!    `wasmer.h` and the `wasmer.hh` C and C++ header files,
//! 2. [`wasm_c_api`], which is the new standard C API, and is
//!    represented by the `wasmer_wasm.h` C header file.
//!
//! The `wasm_c_api` follows the [official WebAssembly C
//! API](https://github.com/WebAssembly/wasm-c-api). This standard can
//! be characterized as a _living standard_. The API is not yet
//! stable, even though it shows maturity over time. It is described
//! by the `wasm.h` C header file. However, the `wasm_c_api` API
//! provides some extensions, like the `wasi_*` or `wasmer_*` types
//! and functions, which aren't yet defined by the standard. The
//! `wasmer_wasm.h` header file already depends on the `wasm.h`
//! file. A copy lands in this repository for the sake of simplicity.
//!
//! It is recommended to use the `wasm_c_api` API, despites it is not
//! yet officially stabilized, over the `deprecated` API, which is
//! more stable but less powerful and in a maintainance state.

#![doc(html_favicon_url = "https://wasmer.io/images/icons/favicon-32x32.png")]
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
