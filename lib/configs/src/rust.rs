//! Rust-specific annotations used to interoperate with external tools.

use crate::{Abi, Bindings};
use std::collections::HashMap;
use std::path::PathBuf;

/// The annotation used by `cargo wapm` when it parses the
/// `[package.metadata.wapm]` table in your `Cargo.toml`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Wasmer {
    /// The namespace this package should be published under.
    pub namespace: String,
    /// The name the package should be published under, if it differs from the
    /// crate name.
    pub package: Option<String>,
    /// Extra flags that should be passed to the `wasmer` CLI.
    pub wasmer_extra_flags: Option<String>,
    /// The ABI to use when adding the compiled crate to the package.
    pub abi: Abi,
    /// Filesystem mappings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fs: Option<HashMap<String, PathBuf>>,
    /// Binding declarations for the crate.
    pub bindings: Option<Bindings>,
}
