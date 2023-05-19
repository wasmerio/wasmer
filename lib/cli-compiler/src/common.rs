//! Common module with common used structures across different
//! commands.
use crate::VERSION;
use clap::Parser;
use std::env;
use std::path::PathBuf;

#[derive(Debug, Parser, Clone, Default)]
/// The WebAssembly features that can be passed through the
/// Command Line args.
pub struct WasmFeatures {
    /// Enable support for the SIMD proposal.
    #[clap(long = "enable-simd")]
    pub simd: bool,

    /// Enable support for the threads proposal.
    #[clap(long = "enable-threads")]
    pub threads: bool,

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

/// Get the cache dir
pub fn get_cache_dir() -> PathBuf {
    match env::var("WASMER_CACHE_DIR") {
        Ok(dir) => {
            let mut path = PathBuf::from(dir);
            path.push(VERSION);
            path
        }
        Err(_) => {
            // We use a temporal directory for saving cache files
            let mut temp_dir = env::temp_dir();
            temp_dir.push("wasmer");
            temp_dir.push(VERSION);
            temp_dir
        }
    }
}
