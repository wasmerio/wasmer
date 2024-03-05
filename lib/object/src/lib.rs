//! Object creator for Wasm Compilations.
//!
//! Given a compilation result (this is, the result when calling `Compiler::compile_module`)
//! this exposes functions to create an Object file for a given target.

#![deny(missing_docs, trivial_numeric_casts, unused_extern_crates)]
#![warn(unused_import_braces)]
#![allow(clippy::new_without_default)]
#![warn(
    clippy::float_arithmetic,
    clippy::mut_mut,
    clippy::nonminimal_bool,
    clippy::map_unwrap_or,
    clippy::print_stdout,
    clippy::unicode_not_nfc,
    clippy::use_self
)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

mod error;
mod module;

pub use crate::error::ObjectError;
pub use crate::module::{emit_compilation, emit_data, emit_serialized, get_object_for_target};
pub use object::{self, write::Object};
