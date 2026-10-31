//! Side-module metadata, link plans and host initialization staging for WASIX.
//!
//! Engine-free plans contain relative addresses, indices and validated bounded effects.
//! Native staging compiles no-auto-start modules without executing guest helpers. The native
//! image owner connects the existing four imports and coordinates all-store publication;
//! libc calls constructors afterward. Managed runner scopes retain cleanup custody and
//! logically retire mappings before retained Rust metadata handles drop; see README.md
//! for the native profile, executor lifetime contract and deferred acceptance limits.

mod image;
#[cfg(feature = "sys")]
mod runtime;
#[cfg(feature = "sys")]
pub use runtime::Native;
#[cfg(feature = "sys")]
mod compile;
#[cfg(feature = "sys")]
pub use compile::{Compile, load};
mod initialize;
mod metadata;
mod plan;
#[cfg(feature = "sys")]
mod store;
#[cfg(feature = "sys")]
pub use store::StagedModule;

pub use image::{SOURCE_SECTION, prepare_image};
pub use initialize::{InitEffects, InitTable, InitWrite, Initialization};
pub use metadata::{Export, ExternType, Import, Module};
pub use plan::{
    BindingMode, BindingTarget, DeferredCheck, Fixup, FixupKind, Layout, Plan, PlanError,
};

#[cfg(test)]
mod tests;
