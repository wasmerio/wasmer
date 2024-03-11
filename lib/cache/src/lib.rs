//! The `wasmer-cache` crate provides the necessary abstractions
//! to cache WebAssembly Modules easily.

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

mod cache;
mod filesystem;
mod hash;

pub use crate::cache::Cache;
#[cfg(feature = "filesystem")]
pub use crate::filesystem::FileSystemCache;
pub use crate::hash::Hash;

// We re-export those for convinience of users
pub use wasmer::{DeserializeError, SerializeError};
