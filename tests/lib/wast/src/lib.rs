//! Implementation of the WAST text format for wasmer.

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

mod error;
mod spectest;
mod wasi_wast;
mod wast;

pub use crate::error::{DirectiveError, DirectiveErrors};
pub use crate::spectest::spectest_importobject;
pub use crate::wasi_wast::{WasiFileSystemKind, WasiTest};
pub use crate::wast::Wast;

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
