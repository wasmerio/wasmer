use std::{
    collections::HashMap,
    sync::Arc
};

use wasmer::{FunctionEnvMut, Store};
use wasmer_vbus::{SpawnOptionsConfig, BusSpawnedProcess};
use wasmer_wasi_types::__WASI_ENOENT;

use crate::{WasiEnv, syscalls::stderr_write, bin_factory::CachedCompiledModules, WasiRuntimeImplementation};
mod cmd_wasmer;

pub trait BuiltInCommand
where Self: std::fmt::Debug {
    fn exec<'a>(&self, parent_ctx: &FunctionEnvMut<'a, WasiEnv>, name: &str, store: Store, config: SpawnOptionsConfig<WasiEnv>) -> wasmer_vbus::Result<BusSpawnedProcess>;
}

#[derive(Debug, Clone)]
pub struct BuiltIns {
    commands: HashMap<String, Arc<dyn BuiltInCommand + Send + Sync + 'static>>,
    pub(crate) cmd_wasmer: cmd_wasmer::CmdWasmer,
}

impl BuiltIns {
    pub(crate) fn new(runtime: Arc<dyn WasiRuntimeImplementation + Send + Sync + 'static>, compiled_modules: Arc<CachedCompiledModules>) -> Self {
        let cmd_wasmer = cmd_wasmer::CmdWasmer::new(
            runtime.clone(),
            compiled_modules.clone());
        let mut commands: HashMap<String, Arc<dyn BuiltInCommand + Send + Sync + 'static>>
            = HashMap::new();
        commands.insert("/bin/wasmer".to_string(), Arc::new(
            cmd_wasmer.clone())
        );

        Self {
            commands,
            cmd_wasmer
        }
    }
}

impl BuiltIns
{
    pub fn exists(&self, name: &str) -> bool {
        let name = name.to_string();
        self.commands.contains_key(&name)
    }

    pub fn exec<'a>(&self, parent_ctx: &FunctionEnvMut<'a, WasiEnv>, name: &str, store: Store, config: SpawnOptionsConfig<WasiEnv>) -> wasmer_vbus::Result<BusSpawnedProcess> {
        let name = name.to_string();
        if let Some(cmd) = self.commands.get(&name) {
            cmd.exec(parent_ctx, name.as_str(), store, config)
        } else {
            let _ = stderr_write(parent_ctx, format!("wasm command unknown - {}\r\n", name).as_bytes());
            Ok(BusSpawnedProcess::exited_process(__WASI_ENOENT as u32))
        }
    }
}
