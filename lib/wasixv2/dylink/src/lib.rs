//! Side-module metadata, link plans and host initialization staging for WASIX.
//!
//! Engine-free plans contain relative addresses, indices and validated bounded effects.
//! Staging compiles reusable no-auto-start modules without executing guest helpers. The
//! image owner connects the existing four imports and coordinates all-store publication;
//! libc calls constructors afterward. Managed runner scopes retain cleanup custody and
//! logically retire mappings before retained Rust metadata handles drop; see README.md
//! for the supported profiles, executor lifetime contract and deferred acceptance limits.

#[cfg(any(feature = "sys", feature = "js"))]
mod artifact;
mod image;
#[cfg(any(feature = "sys", feature = "js"))]
mod runtime;
#[cfg(any(feature = "sys", feature = "js"))]
pub use runtime::{LoadPolicy, Native, compose};
#[cfg(any(feature = "sys", feature = "js"))]
mod compile;
#[cfg(any(feature = "sys", feature = "js"))]
pub use compile::{Compile, load};
mod initialize;
mod metadata;
mod plan;
#[cfg(any(feature = "sys", feature = "js"))]
mod store;
#[cfg(any(feature = "sys", feature = "js"))]
pub use store::StagedModule;

pub use image::{SOURCE_SECTION, needs_preparation, prepare_image};
pub use initialize::{InitEffects, InitTable, InitWrite, Initialization};
pub use metadata::{Export, ExternType, Import, Module};
pub use plan::{
    BindingMode, BindingTarget, DeferredCheck, Fixup, FixupKind, Layout, Plan, PlanError,
};

#[cfg(test)]
mod tests;
