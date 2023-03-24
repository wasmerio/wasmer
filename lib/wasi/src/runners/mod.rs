mod container;
mod runner;

#[cfg(feature = "webc_runner_rt_emscripten")]
pub mod emscripten;
#[cfg(feature = "webc_runner_rt_wasi")]
pub mod wasi;
#[cfg(any(feature = "webc_runner_rt_wasi", feature = "webc_runner_rt_wcgi"))]
mod wasi_common;
#[cfg(feature = "webc_runner_rt_wcgi")]
pub mod wcgi;

pub use self::{
    container::{Bindings, WapmContainer, WebcParseError, WitBindings},
    runner::Runner,
};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MappedDirectory {
    pub host: std::path::PathBuf,
    pub guest: String,
}
