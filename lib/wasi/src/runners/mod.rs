mod container;
mod runner;

#[cfg(feature = "webc_runner_rt_emscripten")]
pub mod emscripten;
#[cfg(feature = "webc_runner_rt_wasi")]
pub mod wasi;
#[cfg(feature = "webc_runner_rt_wcgi")]
pub mod wcgi;

pub use self::{
    container::{Bindings, WapmContainer, WebcParseError, WitBindings},
    runner::Runner,
};

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MappedDirectory {
    pub host: PathBuf,
    pub guest: String,
}
