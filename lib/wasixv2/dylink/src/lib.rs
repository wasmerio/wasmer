//! Side-module metadata, link plans and host initialization staging for WASIX.
//!
//! Engine-free plans contain relative addresses, indices and validated bounded effects.
//! Native staging can compile and instantiate their no-auto-start form, without executing
//! guest helpers or constructors. The process image owner must publish every store together;
//! this crate's staging primitives alone do not implement the four `dl_*` imports.

mod image;
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
