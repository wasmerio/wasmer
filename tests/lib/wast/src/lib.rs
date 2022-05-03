//! Implementation of the WAST text format for wasmer.

#![deny(missing_docs, trivial_numeric_casts, unused_extern_crates)]
#![warn(unused_import_braces)]
#![deny(unstable_features)]
#![cfg_attr(feature = "cargo-clippy", allow(clippy::new_without_default))]
#![cfg_attr(
    feature = "cargo-clippy",
    warn(
        clippy::float_arithmetic,
        clippy::mut_mut,
        clippy::nonminimal_bool,
        clippy::map_unwrap_or,
        clippy::print_stdout,
        clippy::unicode_not_nfc,
        clippy::use_self
    )
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
