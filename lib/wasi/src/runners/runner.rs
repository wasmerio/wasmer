use anyhow::Error;
use webc::{metadata::Command, Container};

/// Trait that all runners have to implement
pub trait Runner {
    /// Returns whether the Runner will be able to run the `Command`
    fn can_run_command(command: &Command) -> Result<bool, Error>
    where
        Self: Sized;

    /// Implementation to run the given command
    ///
    /// - use `cmd.annotations` to get the metadata for the given command
    /// - use `container.get_atom()` to get the
    fn run_command(&mut self, command_name: &str, container: &Container) -> Result<(), Error>;
}
