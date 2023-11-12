#[cfg(feature = "webc_runner_rt_emscripten")]
pub mod emscripten;
pub mod wasi;
mod wasi_common;
#[cfg(feature = "webc_runner_rt_wcgi")]
pub mod wcgi;

pub use self::wasi_common::MappedCommand;
pub use crate::runtime::runner::Runner;

/// A directory that should be mapped from the host filesystem into a WASI
/// instance (the "guest").
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MappedDirectory {
    /// The absolute path for a directory on the host filesystem.
    pub host: std::path::PathBuf,
    /// The absolute path specifying where the host directory should be mounted
    /// inside the guest.
    pub guest: String,
}
