use std::error::Error as StdError;

use webc::v1::Command;

use crate::runners::WapmContainer;

/// Trait that all runners have to implement
pub trait Runner {
    /// The return value of the output of the runner
    type Output;

    /// Returns whether the Runner will be able to run the `Command`
    fn can_run_command(
        &self,
        command_name: &str,
        command: &Command,
    ) -> Result<bool, Box<dyn StdError>>;

    /// Implementation to run the given command
    ///
    /// - use `cmd.annotations` to get the metadata for the given command
    /// - use `container.get_atom()` to get the
    fn run_command(
        &mut self,
        command_name: &str,
        cmd: &Command,
        container: &WapmContainer,
    ) -> Result<Self::Output, Box<dyn StdError>>;

    /// Runs the container if the container has an `entrypoint` in the manifest
    fn run(&mut self, container: &WapmContainer) -> Result<Self::Output, Box<dyn StdError>> {
        let cmd = match container.manifest().entrypoint.as_ref() {
            Some(s) => s,
            None => {
                let path = format!("{}", container.v1().path.display());
                return Err(Box::new(webc::v1::Error(format!(
                    "Cannot run {path:?}: not executable (no entrypoint in manifest)"
                ))));
            }
        };

        self.run_cmd(container, cmd)
    }

    /// Runs the given `cmd` on the container
    fn run_cmd(
        &mut self,
        container: &WapmContainer,
        cmd: &str,
    ) -> Result<Self::Output, Box<dyn StdError>> {
        let webc = container.v1();
        let path = format!("{}", webc.path.display());
        let command_to_exec = webc
            .manifest
            .commands
            .get(cmd)
            .ok_or_else(|| anyhow::anyhow!("{path}: command {cmd:?} not found in manifest"))?;

        let _path = format!("{}", webc.path.display());

        match self.can_run_command(cmd, command_to_exec) {
            Ok(true) => {}
            Ok(false) => {
                return Err(Box::new(webc::v1::Error(format!(
                    "Cannot run command {cmd:?} with runner {:?}",
                    command_to_exec.runner
                ))));
            }
            Err(e) => {
                return Err(Box::new(webc::v1::Error(format!(
                    "Cannot run command {cmd:?} with runner {:?}: {e}",
                    command_to_exec.runner
                ))));
            }
        }

        self.run_command(cmd, command_to_exec, container)
    }
}
