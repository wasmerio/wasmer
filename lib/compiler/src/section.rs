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

/// Custom section Protection.
///
/// Determines how a custom section may be used.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum CustomSectionProtection {
    /// A custom section with read permission.
    Read,
    // We don't include `ReadWrite` here because it would complicate freeze
    // and resumption of executing Modules.

    // We also currently don't include `ReadExecute` as we don't have a way
    // to represent relocations for this kind of section.

    // Singlepass requires `ReadExecute`. Adding it for now.
    /// A custom section with read and write permissions.
    ReadExecute,
}

/// A Section for a `Compilation`.
///
/// This is used so compilers can store arbitrary information
/// in the emitted module.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct CustomSection {
    /// The protection
    pub protection: CustomSectionProtection,
    /// The bytes corresponding to this section.
    ///
    /// > Note: These bytes have to be at-least 8-byte aligned
    /// > (the start of the memory pointer).
    /// > We might need to create another field for alignment in case it's
    /// > needed in the future.
    pub bytes: SectionBody,
}

/// The bytes in the section.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct SectionBody(#[serde(with = "serde_bytes")] Vec<u8>);

impl SectionBody {
    /// Extend the section with the bytes given.
    pub fn extend(&mut self, contents: &[u8]) {
        self.0.extend(contents);
    }

    /// Extends the section by appending bytes from another section.
    pub fn append(&mut self, body: &SectionBody) {
        self.0.extend(&body.0);
    }

    /// Returns a raw pointer to the section's buffer.
    pub fn as_ptr(&self) -> *const u8 {
        self.0.as_ptr()
    }

    /// Returns the length of this section in bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }
}
