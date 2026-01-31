#![allow(clippy::result_large_err)]
use std::{
    collections::HashMap,
    future::Future,
    ops::Deref,
    pin::Pin,
    sync::{Arc, RwLock},
};

use anyhow::Context;
use shared_buffer::OwnedBuffer;
use wasmer::FunctionEnvMut;
use wasmer_package::utils::from_bytes;

mod binary_package;
mod exec;

pub use self::{
    binary_package::*,
    exec::{
        package_command_by_name, run_exec, spawn_exec, spawn_exec_module, spawn_exec_wasm,
        spawn_load_module, spawn_union_fs,
    },
};
use crate::{
    Runtime, SpawnError, WasiEnv,
    fs::WasiFs,
    os::{command::Commands, task::TaskJoinHandle},
    runtime::module_cache::HashedModuleData,
};

#[derive(Debug, Clone)]
pub struct BinFactory {
    pub(crate) commands: Commands,
    runtime: Arc<dyn Runtime + Send + Sync + 'static>,
    pub(crate) local: Arc<RwLock<HashMap<String, Option<Arc<BinaryPackage>>>>>,
}

impl BinFactory {
    pub fn new(runtime: Arc<dyn Runtime + Send + Sync + 'static>) -> BinFactory {
        BinFactory {
            commands: Commands::new_with_builtins(runtime.clone()),
            runtime,
            local: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn runtime(&self) -> &(dyn Runtime + Send + Sync) {
        self.runtime.deref()
    }

    pub fn set_binary(&self, name: &str, binary: &Arc<BinaryPackage>) {
        let mut cache = self.local.write().unwrap();
        cache.insert(name.to_string(), Some(binary.clone()));
    }

    #[allow(clippy::await_holding_lock)]
    pub async fn get_binary(&self, name: &str, fs: Option<&WasiFs>) -> Option<Arc<BinaryPackage>> {
        self.get_executable(name, fs)
            .await
            .and_then(|executable| match executable {
                Executable::Wasm(_) => None,
                Executable::BinaryPackage(pkg) => Some(pkg),
            })
    }

    pub fn spawn<'a>(
        &'a self,
        name: String,
        env: WasiEnv,
    ) -> Pin<Box<dyn Future<Output = Result<TaskJoinHandle, SpawnError>> + 'a>> {
        Box::pin(async move {
            // Find the binary (or die trying) and make the spawn type
            let res = self
                .get_executable(name.as_str(), Some(env.fs()))
                .await
                .ok_or_else(|| SpawnError::BinaryNotFound {
                    binary: name.clone(),
                });
            let executable = res?;

            // Execute
            match executable {
                Executable::Wasm(bytes) => {
                    let data = HashedModuleData::new(bytes.clone());
                    spawn_exec_wasm(data, name.as_str(), env, &self.runtime).await
                }
                Executable::BinaryPackage(pkg) => {
                    {
                        let cmd = package_command_by_name(&pkg, name.as_str())?;
                        env.prepare_spawn(cmd);
                    }

                    spawn_exec(pkg.as_ref().clone(), name.as_str(), env, &self.runtime).await
                }
            }
        })
    }

    pub fn try_built_in(
        &self,
        name: String,
        parent_ctx: Option<&FunctionEnvMut<'_, WasiEnv>>,
        builder: &mut Option<WasiEnv>,
    ) -> Result<TaskJoinHandle, SpawnError> {
        // We check for built in commands
        if let Some(parent_ctx) = parent_ctx {
            if self.commands.exists(name.as_str()) {
                return self.commands.exec(parent_ctx, name.as_str(), builder);
            }
        } else if self.commands.exists(name.as_str()) {
            tracing::warn!("builtin command without a parent ctx - {}", name);
        }
        Err(SpawnError::BinaryNotFound { binary: name })
    }

    // TODO: remove allow once BinFactory is refactored
    // currently fine because a BinFactory is only used by a single process tree
    #[allow(clippy::await_holding_lock)]
    pub async fn get_executable(&self, name: &str, fs: Option<&WasiFs>) -> Option<Executable> {
        let name = name.to_string();

        // Return early if the path is already cached
        {
            let cache = self.local.read().unwrap();
            if let Some(data) = cache.get(&name) {
                data.clone().map(Executable::BinaryPackage);
            }
        }

        let mut cache = self.local.write().unwrap();

        // Check the cache again to avoid a race condition where the cache was populated inbetween the fast path and here
        if let Some(data) = cache.get(&name) {
            return data.clone().map(Executable::BinaryPackage);
        }

        // Check the filesystem for the file
        if name.starts_with('/')
            && let Some(fs) = fs
        {
            match load_executable_from_filesystem(fs, name.as_ref(), self.runtime()).await {
                Ok(executable) => {
                    if let Executable::BinaryPackage(pkg) = &executable {
                        cache.insert(name, Some(pkg.clone()));
                    }

                    return Some(executable);
                }
                Err(e) => {
                    tracing::warn!(
                        path = name,
                        error = &*e,
                        "Unable to load the package from disk"
                    );
                }
            }
        }

        // NAK
        cache.insert(name, None);
        None
    }
}

pub enum Executable {
    Wasm(OwnedBuffer),
    BinaryPackage(Arc<BinaryPackage>),
}

async fn load_executable_from_filesystem(
    fs: &WasiFs,
    path: &str,
    rt: &(dyn Runtime + Send + Sync),
) -> Result<Executable, anyhow::Error> {
    use vfs_core::VfsBaseDirAsync;
    use vfs_core::flags::{OpenFlags, OpenOptions, ResolveFlags};
    use vfs_core::path_types::VfsPath;

    let ctx = fs.ctx.read().unwrap();
    let handle = fs
        .vfs
        .openat_async(
            &ctx,
            VfsBaseDirAsync::Cwd,
            VfsPath::new(path.as_bytes()),
            OpenOptions {
                flags: OpenFlags::READ,
                mode: None,
                resolve: ResolveFlags::empty(),
            },
        )
        .await
        .map_err(|err| anyhow::anyhow!(err).context("Unable to open the file"))?;

    let meta = handle.get_metadata().await.context("metadata")?;
    let mut data = vec![0u8; meta.size as usize];
    let mut read = 0usize;
    while read < data.len() {
        let n = handle
            .read(&mut data[read..])
            .await
            .context("Read failed")?;
        if n == 0 {
            break;
        }
        read += n;
    }
    data.truncate(read);

    if wasmer_package::utils::is_container(&data) {
        let bytes = data.into();
        let container = from_bytes(bytes)?;
        let pkg = BinaryPackage::from_webc(&container, rt)
            .await
            .context("Unable to load the package")?;

        Ok(Executable::BinaryPackage(Arc::new(pkg)))
    } else {
        Ok(Executable::Wasm(OwnedBuffer::from_bytes(data.into())))
    }
}
