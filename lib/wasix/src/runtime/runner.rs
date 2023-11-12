use std::sync::Arc;

use anyhow::Error;
use webc::metadata::Command;

use crate::{bin_factory::BinaryPackage, Runtime};

/// Trait that all runners have to implement
pub trait Runner {
    /// Returns whether the Runner will be able to run the `Command`
    fn can_run_command(command: &Command) -> Result<bool, Error>
    where
        Self: Sized;

    /// Run a command.
    fn run_command(
        &mut self,
        command_name: &str,
        pkg: &BinaryPackage,
        runtime: Arc<dyn Runtime + Send + Sync>,
    ) -> Result<(), Error>;
}
