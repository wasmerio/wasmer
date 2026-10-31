//! Side-module metadata, link plans and host initialization staging for WASIX.
//!
//! Engine-free plans contain relative addresses, indices and validated bounded effects.
//! Native staging compiles no-auto-start modules without executing guest helpers. The native
//! image owner connects the existing four imports and coordinates all-store publication;
//! libc calls constructors afterward. This WIP checkpoint still needs guaranteed async
//! retirement custody before integration; see README.md for profile and lifecycle limits.

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
