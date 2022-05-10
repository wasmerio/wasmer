//! Generic Artifact abstraction for Wasmer Engines.

#![deny(missing_docs, trivial_numeric_casts, unused_extern_crates)]
#![warn(unused_import_braces)]
#![cfg_attr(
    feature = "cargo-clippy",
    allow(clippy::new_without_default, clippy::new_without_default)
)]
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

mod artifact;
mod funcbody;

pub use crate::artifact::{ArtifactCreate, MetadataHeader, Upcastable};
pub use crate::funcbody::VMFunctionBody;

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A safe wrapper around `VMFunctionBody`.
#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct FunctionBodyPtr(pub *const VMFunctionBody);

impl std::ops::Deref for FunctionBodyPtr {
    type Target = *const VMFunctionBody;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// # Safety
/// The VMFunctionBody that this points to is opaque, so there's no data to
/// read or write through this pointer. This is essentially a usize.
unsafe impl Send for FunctionBodyPtr {}
/// # Safety
/// The VMFunctionBody that this points to is opaque, so there's no data to
/// read or write through this pointer. This is essentially a usize.
unsafe impl Sync for FunctionBodyPtr {}
