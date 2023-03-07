use anyhow::Error;
use webc::metadata::Command;

use crate::runners::WapmContainer;

/// Trait that all runners have to implement
pub trait Runner {
    /// The return value of the output of the runner
    type Output;

    /// Returns whether the Runner will be able to run the `Command`
    fn can_run_command(&self, command_name: &str, command: &Command) -> Result<bool, Error>;

    /// Implementation to run the given command
    ///
    /// - use `cmd.annotations` to get the metadata for the given command
    /// - use `container.get_atom()` to get the
    fn run_command(
        &mut self,
        command_name: &str,
        cmd: &Command,
        container: &WapmContainer,
    ) -> Result<Self::Output, Error>;

    /// Runs the container if the container has an `entrypoint` in the manifest
    fn run(&mut self, container: &WapmContainer) -> Result<Self::Output, Error> {
        let cmd = match container.manifest().entrypoint.as_ref() {
            Some(s) => s,
            None => {
                anyhow::bail!("Cannot run the package: not executable (no entrypoint in manifest)");
            }
        };

        self.run_cmd(container, cmd)
    }

    /// Runs the given `cmd` on the container
    fn run_cmd(&mut self, container: &WapmContainer, cmd: &str) -> Result<Self::Output, Error> {
        let command_to_exec = container
            .manifest()
            .commands
            .get(cmd)
            .ok_or_else(|| anyhow::anyhow!("command {cmd:?} not found in manifest"))?;

        match self.can_run_command(cmd, command_to_exec) {
            Ok(true) => {}
            Ok(false) => {
                anyhow::bail!(
                    "Cannot run command {cmd:?} with runner {:?}",
                    command_to_exec.runner
                );
            }
            Err(e) => {
                anyhow::bail!(
                    "Cannot run command {cmd:?} with runner {:?}: {e}",
                    command_to_exec.runner
                );
            }
        }

        self.run_command(cmd, command_to_exec, container)
    }
}
