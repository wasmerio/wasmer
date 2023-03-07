mod handler;
mod runner;

use std::path::PathBuf;

pub use self::runner::{Callbacks, Config, WcgiRunner};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MappedDirectory {
    pub host: PathBuf,
    pub guest: String,
}
