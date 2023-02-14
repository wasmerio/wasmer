use std::{collections::HashMap, ops::Deref, sync::Arc};

use anyhow::Context as _;

use wasmer::{FunctionEnvMut, Store};
use wasmer_vfs::{AsyncReadExt, FileSystem};

mod binary_package;
mod exec;

use wasmer_wasi_types::wasi::ExitCode;

pub use self::{
    binary_package::*,
    exec::{spawn_exec, spawn_exec_command, spawn_exec_module},
};
use crate::{
    os::command::Commands,
    vbus::{BusSpawnedProcess, VirtualBusError},
    WasiEnv, WasiRuntime, WasiRuntimeError, WasiStateCreationError,
};

#[derive(Debug, Clone)]
pub struct BinFactory {
    commands: Commands,
    local: Arc<tokio::sync::RwLock<HashMap<String, Arc<BinaryPackage>>>>,

    runtime: Arc<dyn WasiRuntime + Send + Sync + 'static>,
}

impl BinFactory {
    pub fn new(runtime: Arc<dyn WasiRuntime + Send + Sync>) -> BinFactory {
        BinFactory {
            commands: Commands::new(),
            runtime,
            local: Default::default(),
        }
    }

    pub fn runtime(&self) -> &dyn WasiRuntime {
        self.runtime.deref()
    }

    pub fn set_binary(&self, name: &str, binary: Arc<BinaryPackage>) {
        let mut cache = self.local.blocking_write();
        cache.insert(name.to_string(), binary);
    }

    /// Retrieve a binary pacakge from the local cache or supplied file system.
    pub async fn get_binary(
        &self,
        name: &str,
        fs: Option<&dyn FileSystem>,
    ) -> Result<Option<Arc<BinaryPackage>>, anyhow::Error> {
        let name = name.to_string();

        // Fast path
        {
            let cache = self.local.read().await;
            if let Some(data) = cache.get(&name) {
                return Ok(Some(data.clone()));
            }
        }

        // Check the filesystem for the file
        if name.starts_with('/') {
            if let Some(fs) = fs {
                let mut cache = self.local.write().await;

                if let Ok(mut file) = fs.new_open_options().read(true).open(name.clone()) {
                    // Read the file
                    let mut data = Vec::with_capacity(file.size() as usize);
                    // TODO: log error?
                    file.read_to_end(&mut data)
                        .await
                        .with_context(|| format!("Failed to read binary file '{}'", name))?;

                    let package_name = name.split('/').last().unwrap_or(name.as_str());
                    let data = Arc::new(BinaryPackage::new(package_name, Some(data.into())));
                    cache.insert(name, data.clone());
                    return Ok(Some(data));
                }
            }
        }

        Ok(None)
    }

    /// Resolve a binary package.
    ///
    /// Checks, in this order:
    /// - the local cache
    /// - the given file system
    /// - remote registry via [`crate::runtime::ModuleResolver`].
    pub async fn resolve_binary(
        &self,
        name: &str,
        fs: Option<&dyn FileSystem>,
    ) -> Result<Option<Arc<BinaryPackage>>, anyhow::Error> {
        if let Some(pkg) = self.get_binary(name, fs).await? {
            Ok(Some(pkg))
        } else if let Some(resolver) = self.runtime().module_resolver() {
            if let Some(pkg) = resolver.resolve_binpackage(name).await? {
                self.local
                    .write()
                    .await
                    .insert(name.to_string(), pkg.clone());
                Ok(Some(pkg))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    /// Resolve a binary package.
    ///
    /// Checks, in this order:
    /// - the local cache
    /// - the given file system
    /// - remote registry via [`crate::runtime::ModuleResolver`].
    pub async fn must_resolve_binary(
        &self,
        name: &str,
        fs: Option<&dyn FileSystem>,
    ) -> Result<Arc<BinaryPackage>, WasiRuntimeError> {
        self.resolve_binary(name, fs)
            .await
            .map_err(|err| {
                // TODO: better error mapping
                WasiRuntimeError::Init(WasiStateCreationError::WasiInheritError(format!(
                    "Failed to resolve package '{name}': {err}",
                )))
            })?
            .ok_or_else(|| {
                WasiRuntimeError::Init(WasiStateCreationError::WasiInheritError(format!(
                    "Package not found: '{name}'",
                )))
            })
    }

    pub async fn spawn_name(
        &self,
        name: &str,
        store: Store,
        env: WasiEnv,
    ) -> Result<BusSpawnedProcess, WasiRuntimeError> {
        spawn_exec_command(name, store, env).await
    }

    pub fn spawn(
        &self,
        pkg: Arc<BinaryPackage>,
        store: Store,
        env: WasiEnv,
    ) -> Result<BusSpawnedProcess, WasiRuntimeError> {
        spawn_exec(pkg, store, env)
    }

    pub fn has_command(&self, path: &str) -> bool {
        self.commands.exists(path)
    }

    pub fn try_built_in(
        &self,
        name: String,
        parent_ctx: Option<&FunctionEnvMut<'_, WasiEnv>>,
        store: Store,
        builder: WasiEnv,
    ) -> Result<BusSpawnedProcess, VirtualBusError> {
        // We check for built in commands
        if let Some(parent_ctx) = parent_ctx {
            if self.commands.exists(name.as_str()) {
                return self
                    .commands
                    .exec(parent_ctx, name.as_str(), store, builder);
            }
        } else if self.commands.exists(name.as_str()) {
            tracing::warn!("builtin command without a parent ctx - {}", name);
        }
        Err(VirtualBusError::NotFound)
    }

    pub fn commands(&self) -> &Commands {
        &self.commands
    }
}

pub type InstanceResult = Result<ExitCode, Arc<WasiRuntimeError>>;

/// Status of a spawned instance.
#[derive(Debug, Clone)]
pub enum InstanceStatus {
    /// Instance was provisioned, but has not started executing yet.
    Pending,
    /// Instance is running.
    Running,
    /// Instance has exited, either with a valid exit code or with a runtime error.
    Finished(InstanceResult),
    /// The connection to the instance was lost before the exit result was recorded.
    ConnectionLost,
}

impl InstanceStatus {
    pub fn to_result(&self) -> Option<InstanceResult> {
        match self {
            Self::Pending | Self::Running => None,
            Self::Finished(res) => Some(res.clone()),
            Self::ConnectionLost => Some(Err(Arc::new(WasiRuntimeError::ControlPlane(
                crate::os::task::control_plane::ControlPlaneError::TaskAborted,
            )))),
        }
    }
}

impl InstanceStatus {
    /// Returns `true` if the instance status is [`Running`].
    ///
    /// [`Running`]: InstanceStatus::Running
    #[must_use]
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }

    /// Returns `true` if the instance status is [`Finished`].
    ///
    /// [`Finished`]: InstanceStatus::Finished
    #[must_use]
    pub fn is_finished(&self) -> bool {
        matches!(self, Self::Finished(..))
    }

    pub fn as_finished(&self) -> Option<&InstanceResult> {
        if let Self::Finished(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

struct SpawnResultSender {
    sender: tokio::sync::watch::Sender<InstanceStatus>,
}

impl SpawnResultSender {
    pub fn on_exit(self, code: ExitCode) {
        self.sender.send(InstanceStatus::Finished(Ok(code)));
    }

    pub fn on_failure(self, err: WasiRuntimeError) {
        self.sender
            .send(InstanceStatus::Finished(Err(Arc::new(err))));
    }
}

#[derive(Debug)]
pub struct SpawnedInstance {
    receiver: tokio::sync::watch::Receiver<InstanceStatus>,
}

impl SpawnedInstance {
    pub fn new() -> (SpawnResultSender, Self) {
        let (sender, exit) = tokio::sync::watch::channel(InstanceStatus::Running);
        (SpawnResultSender { sender }, Self { receiver: exit })
    }

    pub fn status(&self) -> InstanceStatus {
        self.receiver.borrow().clone()
    }

    pub async fn wait(&mut self) -> InstanceResult {
        if let Some(res) = self.receiver.borrow().to_result() {
            return res;
        }

        loop {
            if let Err(err) = self.receiver.changed().await {
                return InstanceStatus::ConnectionLost.to_result().unwrap();
            }
            if let Some(res) = self.receiver.borrow().to_result() {
                return res;
            }
        }
    }
}
