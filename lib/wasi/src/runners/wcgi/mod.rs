mod handler;
mod runner;

use std::path::PathBuf;

pub use self::runner::{Config, WcgiRunner, Callbacks};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MappedDirectory {
    pub host: PathBuf,
    pub guest: String,
}
