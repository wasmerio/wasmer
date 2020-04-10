use std::env;
use std::path::PathBuf;
use structopt::StructOpt;
use wasmer_runtime::{Features, VERSION};

#[derive(Debug, StructOpt, Clone)]
pub struct PrestandardFeatures {
    /// Enable support for the SIMD proposal.
    #[structopt(long = "enable-simd")]
    pub simd: bool,

    /// Enable support for the threads proposal.
    #[structopt(long = "enable-threads")]
    pub threads: bool,

    /// Enable support for all pre-standard proposals.
    #[structopt(long = "enable-all")]
    pub all: bool,
}

impl PrestandardFeatures {
    /// Generate [`wabt::Features`] struct from CLI options
    #[cfg(feature = "wabt")]
    pub fn into_wabt_features(&self) -> wabt::Features {
        let mut features = wabt::Features::new();
        if self.simd || self.all {
            features.enable_simd();
        }
        if self.threads || self.all {
            features.enable_threads();
        }
        features.enable_sign_extension();
        features.enable_sat_float_to_int();
        features
    }

    /// Generate [`Features`] struct from CLI options
    pub fn into_backend_features(&self) -> Features {
        Features {
            simd: self.simd || self.all,
            threads: self.threads || self.all,
        }
    }
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
