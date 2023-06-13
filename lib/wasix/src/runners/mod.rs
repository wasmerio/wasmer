mod runner;

#[cfg(feature = "webc_runner_rt_emscripten")]
pub mod emscripten;
pub mod wasi;
mod wasi_common;
#[cfg(feature = "webc_runner_rt_wcgi")]
pub mod wcgi;

pub use self::runner::Runner;

/// A directory that should be mapped from the host filesystem into a WASI
/// instance (the "guest").
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MappedDirectory {
    pub host: std::path::PathBuf,
    pub guest: String,
}
