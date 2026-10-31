//! Side-module metadata and link plans for WASIX, without compiling or running guest code.
//!
//! This is the planning half of DESIGN.md's host/libc2 split. Plans contain relative addresses,
//! module-local indices and deferred checks, never live handles or Wasmer functions. They do
//! not authenticate initialization helpers, allocate guest pages, instantiate modules, run
//! constructors or implement the four `dl_*` imports. The eventual image owner must validate
//! and publish a plan together with every store's instances before any guest can use it.

mod initialize;
mod metadata;
mod plan;

pub use initialize::{InitEffects, InitTable, InitWrite, Initialization};
pub use metadata::{Export, ExternType, Import, Module};
pub use plan::{
    BindingMode, BindingTarget, DeferredCheck, Fixup, FixupKind, Layout, Plan, PlanError,
};

#[cfg(test)]
mod tests;
