//! This module define the required structures to emit custom
//! Sections in a `Compilation`.
//!
//! The functions that access a custom [`CustomSection`] would need
//! to emit a custom relocation: `RelocationTarget::CustomSection`, so
//! it can be patched later by the engine (native or JIT).

use crate::std::vec::Vec;
use serde::{Deserialize, Serialize};
use wasm_common::entity::entity_impl;

/// Index type of a Section defined inside a WebAssembly `Compilation`.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct SectionIndex(u32);
entity_impl!(SectionIndex);

/// The kind of section
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum CustomSectionKind {
    /// A custom section with read permissions,
    Read,
    // We are skipping `ReadWrite` because, in the future, we would need
    // to freeze/resume execution of Modules. And for that we need
    // immutable state on the emited code.
    /// A compiledd section that is also executable.
    ReadExecute,
}

/// A Section for a `Compilation`.
///
/// This is used so compilers can store arbitrary information
/// in the emited module.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct CustomSection {
    /// The kind of Section
    pub kind: CustomSectionKind,
    /// The bytes corresponding to this section.
    ///
    /// > Note: This bytes have to be at-least 8-byte aligned
    /// > (the start of the memory pointer).
    /// > We might need to create another field for alignment in case it's
    /// > needed in the future.
    #[serde(with = "serde_bytes")]
    pub bytes: Vec<u8>,
}
