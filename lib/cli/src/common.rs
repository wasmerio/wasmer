//! Common module with common used structures across different
//! commands.

use clap::{Parser, ValueEnum};

#[derive(Debug, Parser, Clone, Default)]
/// The WebAssembly features that can be passed through the
/// Command Line args.
pub struct WasmFeatures {
    /// Enable support for the SIMD proposal.
    #[clap(long = "enable-simd")]
    pub simd: bool,

    /// Disable support for the threads proposal.
    #[clap(long = "disable-threads")]
    pub disable_threads: bool,

    /// Deprecated, threads are enabled by default.
    #[clap(long = "enable-threads")]
    pub _threads: bool,

    /// Enable support for the reference types proposal.
    #[clap(long = "enable-reference-types")]
    pub reference_types: bool,

    /// Enable support for the multi value proposal.
    #[clap(long = "enable-multi-value")]
    pub multi_value: bool,

    /// Enable support for the bulk memory proposal.
    #[clap(long = "enable-bulk-memory")]
    pub bulk_memory: bool,

    /// Enable support for all pre-standard proposals.
    #[clap(long = "enable-all")]
    pub all: bool,
}

pub(crate) fn normalize_path(s: &str) -> String {
    s.strip_prefix(r"\\?\").unwrap_or(s).to_string()
}

/// Hashing algorithm to be used for the module info
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ValueEnum)]
pub enum HashAlgorithm {
    /// Sha256
    Sha256,
    /// XXHash
    XXHash,
}

impl Default for HashAlgorithm {
    fn default() -> Self {
        Self::Sha256
    }
}

impl From<HashAlgorithm> for wasmer_types::HashAlgorithm {
    fn from(value: HashAlgorithm) -> Self {
        match value {
            HashAlgorithm::Sha256 => wasmer_types::HashAlgorithm::Sha256,
            HashAlgorithm::XXHash => wasmer_types::HashAlgorithm::XXHash,
        }
    }
}
