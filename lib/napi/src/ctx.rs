use anyhow::{bail, Context, Result};
use std::collections::{HashMap, VecDeque};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use wasmer::{ExternType, FunctionEnv, Imports, Instance, Module, StoreMut, Table, Value};

#[cfg(feature = "wasix")]
use wasmer_wasix::{runners::wasi::WasiRunner, PluggableRuntime};

use crate::{
    guest::{
        callback::{clear_top_level_callback_state, set_top_level_callback_state},
        napi::{register_env_imports, register_napi_imports},
    },
    RuntimeEnv,
};

#[derive(Debug, Clone, Default)]
pub struct NapiLimits {
    pub max_sessions: Option<usize>,
    pub max_envs: Option<usize>,
    pub max_total_external_memory: Option<u64>,
    pub max_total_heap_bytes: Option<u64>,
}

#[derive(Debug, Default)]
pub struct NapiCtxBuilder {
    limits: NapiLimits,
}

#[derive(Clone, Debug)]
pub struct NapiCtx {
    inner: Arc<NapiCtxInner>,
}

#[derive(Clone)]
pub struct NapiSession {
    inner: Arc<NapiSessionInner>,
}

#[derive(Clone, Debug)]
pub struct NapiRuntimeHooks {
    ctx: NapiCtx,
    sessions: Arc<Mutex<HashMap<usize, VecDeque<NapiSession>>>>,
}

#[derive(Debug)]
struct NapiCtxInner {
    limits: NapiLimits,
    active_sessions: AtomicUsize,
}

struct NapiSessionInner {
    ctx: Arc<NapiCtxInner>,
    imported_memory_type: Option<wasmer::MemoryType>,
    imported_table_type: Option<wasmer::TableType>,
    func_env: Mutex<Option<FunctionEnv<RuntimeEnv>>>,
}

impl std::fmt::Debug for NapiSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NapiSession").finish_non_exhaustive()
    }
}

impl Drop for NapiSessionInner {
    fn drop(&mut self) {
        clear_top_level_callback_state();
        self.ctx.active_sessions.fetch_sub(1, Ordering::AcqRel);
    }
}

impl NapiCtxBuilder {
    pub fn max_sessions(mut self, max_sessions: usize) -> Self {
        self.limits.max_sessions = Some(max_sessions);
        self
    }

    pub fn max_envs(mut self, max_envs: usize) -> Self {
        self.limits.max_envs = Some(max_envs);
        self
    }

    pub fn max_total_external_memory(mut self, bytes: u64) -> Self {
        self.limits.max_total_external_memory = Some(bytes);
        self
    }

    pub fn max_total_heap_bytes(mut self, bytes: u64) -> Self {
        self.limits.max_total_heap_bytes = Some(bytes);
        self
    }

    pub fn build(self) -> NapiCtx {
        NapiCtx {
            inner: Arc::new(NapiCtxInner {
                limits: self.limits,
                active_sessions: AtomicUsize::new(0),
            }),
        }
    }
}

impl Default for NapiCtx {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl NapiCtx {
    pub fn builder() -> NapiCtxBuilder {
        NapiCtxBuilder::default()
    }

    pub fn limits(&self) -> &NapiLimits {
        &self.inner.limits
    }

    pub fn active_sessions(&self) -> usize {
        self.inner.active_sessions.load(Ordering::Acquire)
    }

    pub fn prepare_module(&self, module: &Module) -> Result<NapiSession> {
        self.new_session(module)
    }

    pub fn module_needs_napi(module: &Module) -> bool {
        const NAPI_ENV_IMPORTS: &[&str] = &[
            "uv_cpu_info",
            "uv_interface_addresses",
            "uv_free_interface_addresses",
            "uv_resident_set_memory",
            "uv_get_free_memory",
            "uv_get_total_memory",
            "_Z20OSSL_set_max_threadsP15ossl_lib_ctx_sty",
        ];

        module.imports().any(|import| {
            import.module() == "napi"
                || (import.module() == "env" && NAPI_ENV_IMPORTS.contains(&import.name()))
        })
    }

    pub fn runtime_hooks(&self) -> NapiRuntimeHooks {
        NapiRuntimeHooks {
            ctx: self.clone(),
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn new_session(&self, module: &Module) -> Result<NapiSession> {
        let previous = self.inner.active_sessions.fetch_add(1, Ordering::AcqRel);
        if let Some(max_sessions) = self.inner.limits.max_sessions {
            if previous >= max_sessions {
                self.inner.active_sessions.fetch_sub(1, Ordering::AcqRel);
                bail!("refusing to create more than {max_sessions} active N-API sessions");
            }
        }

        let imported_memory_type = module.imports().find_map(|import| {
            if import.module() == "env" && import.name() == "memory" {
                if let ExternType::Memory(ty) = import.ty() {
                    return Some(*ty);
                }
            }
            None
        });

        let imported_table_type = module.imports().find_map(|import| {
            if import.module() == "env" && import.name() == "__indirect_function_table" {
                if let ExternType::Table(ty) = import.ty() {
                    return Some(*ty);
                }
            }
            None
        });

        Ok(NapiSession {
            inner: Arc::new(NapiSessionInner {
                ctx: Arc::clone(&self.inner),
                imported_memory_type,
                imported_table_type,
                func_env: Mutex::new(None),
            }),
        })
    }

    #[cfg(feature = "wasix")]
    pub fn extend_runtime(&self, runtime: &mut PluggableRuntime) {
        self.runtime_hooks().attach_to_runtime(runtime);
    }

    #[cfg(feature = "wasix")]
    pub fn extend_wasi_runner(
        &self,
        runner: &mut WasiRunner,
        runtime: &mut PluggableRuntime,
        module: &Module,
    ) {
        if Self::module_needs_napi(module) {
            runner
                .capabilities_mut()
                .threading
                .enable_asynchronous_threading = false;
        }
        self.extend_runtime(runtime);
    }

    #[cfg(feature = "wasix")]
    pub fn configure_runtime(
        &self,
        runtime: &mut PluggableRuntime,
        _module: &Module,
    ) -> Result<()> {
        self.extend_runtime(runtime);
        Ok(())
    }
}

impl NapiRuntimeHooks {
    fn module_key(module: &Module) -> usize {
        module as *const Module as usize
    }

    pub fn additional_imports(&self, module: &Module, store: &mut StoreMut<'_>) -> Result<Imports> {
        if !NapiCtx::module_needs_napi(module) {
            return Ok(Imports::new());
        }

        let session = self.ctx.prepare_module(module)?;
        let imports = session.create_imports(store)?;
        let mut sessions = self
            .sessions
            .lock()
            .expect("poisoned NapiRuntimeHooks session queue");
        sessions
            .entry(Self::module_key(module))
            .or_default()
            .push_back(session);
        Ok(imports)
    }

    pub fn configure_instance(
        &self,
        module: &Module,
        store: &mut StoreMut<'_>,
        instance: &Instance,
    ) -> Result<()> {
        if !NapiCtx::module_needs_napi(module) {
            return Ok(());
        }

        let session = {
            let mut sessions = self
                .sessions
                .lock()
                .expect("poisoned NapiRuntimeHooks session queue");
            let key = Self::module_key(module);
            let Some(queue) = sessions.get_mut(&key) else {
                bail!("missing pending N-API session for module instance setup");
            };
            let session = queue
                .pop_front()
                .context("missing queued N-API session for module instance setup")?;
            if queue.is_empty() {
                sessions.remove(&key);
            }
            session
        };

        session.configure_instance(store, instance)
    }

    #[cfg(feature = "wasix")]
    pub fn attach_to_runtime(&self, runtime: &mut PluggableRuntime) {
        let hooks = self.clone();
        runtime
            .with_additional_imports(move |module, store| hooks.additional_imports(module, store));

        let hooks = self.clone();
        runtime.with_instance_setup(move |module, store, instance| {
            hooks.configure_instance(module, store, instance)
        });
    }
}

impl NapiSession {
    pub fn create_imports(&self, store: &mut StoreMut<'_>) -> Result<Imports> {
        let mut import_object = Imports::new();
        register_env_imports(store, &mut import_object);

        let func_env = FunctionEnv::new(store, RuntimeEnv::default());
        {
            let mut guard = self
                .inner
                .func_env
                .lock()
                .expect("poisoned NapiSession mutex");
            *guard = Some(func_env.clone());
        }
        register_napi_imports(store, &func_env, &mut import_object);

        if let Some(memory_type) = self.inner.imported_memory_type {
            let memory = wasmer::Memory::new(&mut *store, memory_type)?;
            import_object.define("env", "memory", memory.clone());
            func_env.as_mut(&mut *store).memory = Some(memory);
        }

        if let Some(table_type) = self.inner.imported_table_type {
            let table = Table::new(&mut *store, table_type, Value::FuncRef(None))?;
            import_object.define("env", "__indirect_function_table", table.clone());
            func_env.as_mut(&mut *store).table = Some(table);
        }

        Ok(import_object)
    }

    pub fn configure_instance(&self, store: &mut StoreMut<'_>, instance: &Instance) -> Result<()> {
        let func_env = {
            let guard = self
                .inner
                .func_env
                .lock()
                .expect("poisoned NapiSession mutex");
            guard
                .clone()
                .context("missing runtime function env during instance setup")?
        };

        for export_name in ["unofficial_napi_guest_malloc", "ubi_guest_malloc", "malloc"] {
            if let Ok(malloc) = instance
                .exports
                .get_typed_function::<i32, i32>(&store, export_name)
            {
                func_env.as_mut(&mut *store).malloc_fn = Some(malloc);
                break;
            }
        }

        if let Ok(table) = instance.exports.get_table("__indirect_function_table") {
            func_env.as_mut(&mut *store).table = Some(table.clone());
        }
        let table = func_env.as_ref(&store).table.clone();
        let guest_envs = func_env.as_ref(&store).napi_state_to_guest_env.clone();
        set_top_level_callback_state(store, table, guest_envs);
        Ok(())
    }

    #[cfg(feature = "wasix")]
    pub fn attach_to_runtime(&self, runtime: &mut PluggableRuntime) {
        let session = self.clone();
        runtime.with_additional_imports(move |_module, store| session.create_imports(store));

        let session = self.clone();
        runtime.with_instance_setup(move |_module, store, instance| {
            session.configure_instance(store, instance)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::NapiCtx;
    use crate::module::make_store;
    use wasmer::Module;

    const EMPTY_WASM_MODULE: &[u8] = b"\0asm\x01\0\0\0";

    #[test]
    fn max_sessions_limit_is_enforced() {
        let store = make_store();
        let module = Module::new(&store, EMPTY_WASM_MODULE).expect("empty wasm module compiles");
        let ctx = NapiCtx::builder().max_sessions(1).build();

        let first = ctx
            .prepare_module(&module)
            .expect("first session should be created");
        assert_eq!(ctx.active_sessions(), 1);
        assert!(ctx.prepare_module(&module).is_err());

        drop(first);
        assert_eq!(ctx.active_sessions(), 0);

        let _second = ctx
            .prepare_module(&module)
            .expect("session slot should be released after drop");
        assert_eq!(ctx.active_sessions(), 1);
    }
}
