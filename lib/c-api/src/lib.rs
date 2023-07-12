//! Wasmer C API.
//!
//! [Wasmer](https://github.com/wasmerio/wasmer) is the leading
//! WebAssembly runtime. Wasmer is written in Rust; this crate
//! exposes its C and C++ bindings.
//!
//! This crate provides an API that follows the [official WebAssembly
//! C API](https://github.com/WebAssembly/wasm-c-api). This standard
//! can be characterized as a _living standard_. The API is not yet
//! stable, even though it shows maturity over time. It is described
//! by the `wasm.h` C header file. However, this crate API provides
//! some extensions, like the `wasi_*` or `wasmer_*` types and
//! functions, which aren't yet defined by the standard. The
//! `wasmer.h` header file already depends on the `wasm.h`
//! file. A copy lands in this repository for the sake of simplicity.

#![doc(html_favicon_url = "https://wasmer.io/images/icons/favicon-32x32.png")]
#![doc(html_logo_url = "https://github.com/wasmerio.png?size=200")]
#![deny(
    dead_code,
    unused_imports,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
// Because this crate exposes a lot of C APIs which are unsafe by definition,
// we allow unsafe without explicit safety documentation for each of them.
#![allow(clippy::missing_safety_doc)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

pub mod error;
pub mod wasm_c_api;
