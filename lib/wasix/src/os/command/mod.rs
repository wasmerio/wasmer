pub mod builtins;

use std::{collections::HashMap, sync::Arc};

use wasmer::FunctionEnvMut;
use wasmer_wasix_types::wasi::Errno;

use crate::{
    runtime::task_manager::InlineWaker, syscalls::stderr_write, Runtime, SpawnError, WasiEnv,
};

use super::task::{OwnedTaskStatus, TaskJoinHandle, TaskStatus};

/// A command available to an OS environment.
pub trait VirtualCommand
where
    Self: std::fmt::Debug,
{
    /// Returns the canonical name of the command.
    fn name(&self) -> &str;

    /// Retrieve the command as as a [`std::any::Any`] reference.
    fn as_any(&self) -> &dyn std::any::Any;

    /// Executes the command.
    fn exec(
        &self,
        parent_ctx: &FunctionEnvMut<'_, WasiEnv>,
        path: &str,
        config: &mut Option<WasiEnv>,
    ) -> Result<TaskJoinHandle, SpawnError>;
}

#[derive(Debug, Clone)]
pub struct Commands {
    commands: HashMap<String, Arc<dyn VirtualCommand + Send + Sync + 'static>>,
}

impl Commands {
    fn new() -> Self {
        Self {
            commands: HashMap::new(),
        }
    }

    // TODO: this method should be somewhere on the runtime, not here.
    pub fn new_with_builtins(runtime: Arc<dyn Runtime + Send + Sync + 'static>) -> Self {
        let mut cmd = Self::new();
        let cmd_wasmer = builtins::cmd_wasmer::CmdWasmer::new(runtime.clone());
        cmd.register_command(cmd_wasmer);

        cmd
    }

    /// Register a command.
    ///
    /// The command will be available with it's canonical name ([`VirtualCommand::name()`]) at /bin/NAME.
    pub fn register_command<C: VirtualCommand + Send + Sync + 'static>(&mut self, cmd: C) {
        let path = format!("/bin/{}", cmd.name());
        self.register_command_with_path(cmd, path);
    }

    /// Register a command at a custom path.
    pub fn register_command_with_path<C: VirtualCommand + Send + Sync + 'static>(
        &mut self,
        cmd: C,
        path: String,
    ) {
        self.commands.insert(path, Arc::new(cmd));
    }

    /// Determine if a command exists at the given path.
    pub fn exists(&self, path: &str) -> bool {
        let name = path.to_string();
        self.commands.contains_key(&name)
    }

    /// Get a command by its path.
    pub fn get(&self, path: &str) -> Option<&Arc<dyn VirtualCommand + Send + Sync + 'static>> {
        self.commands.get(path)
    }

    /// Execute a command.
    pub fn exec(
        &self,
        parent_ctx: &FunctionEnvMut<'_, WasiEnv>,
        path: &str,
        builder: &mut Option<WasiEnv>,
    ) -> Result<TaskJoinHandle, SpawnError> {
        let path = path.to_string();
        if let Some(cmd) = self.commands.get(&path) {
            cmd.exec(parent_ctx, path.as_str(), builder)
        } else {
            unsafe {
                InlineWaker::block_on(stderr_write(
                    parent_ctx,
                    format!("wasm command unknown - {path}\r\n").as_bytes(),
                ))
            }
            .ok();

            let res = OwnedTaskStatus::new(TaskStatus::Finished(Ok(Errno::Noent.into())));
            Ok(res.handle())
        }
    }
}
