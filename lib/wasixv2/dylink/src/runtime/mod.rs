//! Per-image EH/PIC loader. Shared state contains plans/source, never store-local functions.
mod forward;
mod graph;
mod load;
mod policy;
pub use policy::{LoadPolicy, compose};
mod startup;
use crate::{
    BindingTarget, ExternType, FixupKind, SOURCE_SECTION,
    store::{BoundModule, StagedExports},
};
use graph::{Graph, Library, infrastructure, initial_globals};
use std::{
    any::Any,
    collections::{BTreeMap, VecDeque},
    ops::Range,
    sync::{
        Arc, Mutex, MutexGuard, OnceLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
};
use wasixv2_abi::{Cstr, DlFlags, DlHandle, DlHandlePtr, Uptr, UptrPtr};
use wasixv2_exec::{
    Abi, AbiInstance, Entry, EntryReturn, ExecEnv, ExecError, Role, Sigreturn, VforkResume, Wasixv2,
};
use wasixv2_syscalls::{
    Caller, Control, Errno, LoaderAdmission, LoaderGate, LoaderPhase, LoaderReservation,
    LoaderTransaction, LoaderWait, LocalFuture, SignalFrame, SysResult, Syscalls, TaskId,
    UserSliceMut, read_cstr, write_value,
};
use wasmer::{
    AsEngineRef, AsStoreMut, AsyncFunctionEnvMut, Extern, FunctionEnv, Global, Imports, Instance,
    Memory, Module, Mutability, RuntimeError, Store, StoreMut, Table, Type, Value,
};
type Result<T> = std::result::Result<T, RuntimeError>;
fn failure(error: impl std::fmt::Debug) -> RuntimeError {
    RuntimeError::new(format!("{error:?}"))
}
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The same finite host operations, reached from an import or a paused outer call.
enum Context<'a, S: 'static> {
    Import(&'a AsyncFunctionEnvMut<ExecEnv<S>>),
    Entry(&'a Entry<'a, S>),
}
impl<S: Send + Sync + 'static> Context<'_, S> {
    async fn with<R>(&self, f: impl FnOnce(&mut ExecEnv<S>, &mut StoreMut<'_>) -> R) -> R {
        match self {
            Self::Import(env) => {
                let mut guard = env.write().await;
                let (data, store) = guard.data_and_store_mut();
                f(data, &mut store.as_store_mut())
            }
            Self::Entry(entry) => entry.with_store(f).await,
        }
    }
}

struct Servicing<'a>(&'a AtomicBool);
impl Drop for Servicing<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Register before Wasixv2. Only explicitly source-prepared EH/PIC images opt in.
pub struct Native;
impl<S: Syscalls + Control + Send + Sync + 'static> Abi<S> for Native {
    fn name(&self) -> &'static str {
        "wasixv2-dylink"
    }
    fn matches(&self, module: &Module) -> bool {
        module.custom_sections(SOURCE_SECTION).next().is_some()
            && <Wasixv2 as Abi<S>>::matches(&Wasixv2, module)
    }
    fn matches_image(&self, image: &[u8]) -> bool {
        wasixv2_exec::is_wasixv2_module(image)
    }
    fn for_image(&self, module: &Module) -> Option<Arc<dyn Abi<S>>> {
        compose(module, Arc::new(Wasixv2), Arc::new(policy::NativePolicy))
    }

    fn imports(&self, _: &mut Store, _: &FunctionEnv<ExecEnv<S>>, _: &Module) -> Imports {
        Imports::new()
    }
    fn bind(
        &self,
        _: &Instance,
        _: &mut Store,
        _: TaskId,
        _: Role,
    ) -> std::result::Result<Arc<dyn AbiInstance<S>>, ExecError> {
        Err(ExecError::NoAbi)
    }
}
pub(crate) fn verify_compiled_source(module: &Module) -> Result<Vec<u8>> {
    let mut sections = module.custom_sections(SOURCE_SECTION);
    let source = sections
        .next()
        .ok_or_else(|| failure("missing validated source"))?
        .to_vec();
    if sections.next().is_some() {
        return Err(failure("duplicate source marker"));
    }
    #[cfg(feature = "sys")]
    if module.sys_artifact().is_some() {
        let expected = crate::prepare_image(&source)
            .map_err(failure)
            .and_then(|bytes| wasixv2_exec::instrument(&bytes).map_err(failure))?;
        if module.info().hash != Some(wasmer_types::ModuleHash::new(&expected))
            || module.info().start_function.is_some()
            || !module.info().table_initializers.is_empty()
        {
            return Err(failure(
                "compiled image differs from validated no-auto-start source",
            ));
        }
        return Ok(source);
    }
    #[cfg(feature = "js")]
    {
        // As with other JS embedding, the host supplies the matching prepared
        // Module/source. Source validation and transformation stay loader-owned.
        Ok(source)
    }
    #[cfg(not(feature = "js"))]
    {
        Err(failure("dynamic image requires sys or js backend"))
    }
}
type ArtifactKey = (wasmer_types::ModuleHash, Vec<(u32, u32)>, String, String);
struct Root {
    policy: Arc<dyn LoadPolicy>,
    owner: Arc<()>,
    artifacts: Mutex<BTreeMap<ArtifactKey, Arc<crate::artifact::PreparedArtifact>>>,
    forward_code: Arc<forward::Cache>,
    cleanup: Mutex<Vec<Weak<wasixv2_host::LoaderCleanup>>>,
    gate: OnceLock<Arc<LoaderGate>>,
    host: OnceLock<Arc<dyn wasixv2_host::Host>>,
    error: OnceLock<String>,
    initialized: AtomicBool,
    retired: AtomicBool,
    stack: u32,
    state: Mutex<State>,
}
struct State {
    graph: Arc<Graph>,
    work: Option<Arc<Work>>,
    ctors: VecDeque<u32>,
    references: BTreeMap<usize, u32>,
    leases: Vec<Box<dyn LoaderReservation>>,
}
struct Publication {
    graph: Arc<Graph>,
    references: BTreeMap<usize, u32>,
    ctors: VecDeque<u32>,
}
struct Work {
    publication: Mutex<Option<Publication>>,
    previous: usize,
    first: TaskId,
    leases: Mutex<Vec<Box<dyn LoaderReservation>>>,
    committed: AtomicBool,
    error: Mutex<Option<String>>,
}
impl Work {
    fn graph(&self) -> Result<Arc<Graph>> {
        lock(&self.publication)
            .as_ref()
            .map(|p| p.graph.clone())
            .ok_or_else(|| failure("missing prepared graph"))
    }
}
impl Root {
    #[cfg(test)]
    fn new(source: Vec<u8>) -> Arc<Self> {
        Self::with_policy(source, Arc::new(policy::NativePolicy))
    }
    fn with_policy(source: Vec<u8>, policy: Arc<dyn LoadPolicy>) -> Arc<Self> {
        let parsed = (|| -> Result<(Graph, u32)> {
            let mut main = Library::new(source, None)?;
            let stack = main
                .metadata
                .exports()
                .get("__stack_pointer")
                .and_then(|e| e.offset)
                .unwrap_or(8 * 1024 * 1024);
            if stack < 131072 {
                return Err(failure("dynamic stack overlaps low guard"));
            }
            let info = main.metadata.memory_info();
            main.base = align(
                stack,
                1u32.checked_shl(info.memory_alignment)
                    .ok_or_else(|| failure("main alignment"))?,
            )?;
            let mut graph = Graph {
                libraries: vec![main],
                global: vec![0],
                initialization: vec![0],
                cursor: 3u32
                    .checked_add(info.table_size)
                    .ok_or_else(|| failure("main table overflow"))?,
            };
            graph.assign_slots(0)?;
            Ok((graph, stack))
        })();
        let error = OnceLock::new();
        let (graph, stack) = match parsed {
            Ok(value) => value,
            Err(e) => {
                let _ = error.set(e.to_string());
                (
                    Graph {
                        libraries: Vec::new(),
                        global: Vec::new(),
                        initialization: Vec::new(),
                        cursor: 3,
                    },
                    0,
                )
            }
        };
        Arc::new(Self {
            policy,
            owner: Arc::new(()),
            artifacts: Mutex::default(),
            forward_code: Arc::default(),
            cleanup: Mutex::default(),
            gate: OnceLock::new(),
            host: OnceLock::new(),
            error,
            initialized: AtomicBool::new(false),
            retired: AtomicBool::new(false),
            stack,
            state: Mutex::new(State {
                graph: Arc::new(graph),
                work: None,
                ctors: VecDeque::new(),
                references: BTreeMap::from([(0, 1)]),
                leases: Vec::new(),
            }),
        })
    }
    // V1's engine/format key keeps compiled code reusable across compatible stores.
    // Source and validated plans are loader-owned; peers never reparse or compare bytes.
    fn prepare_code(&self, engine: &impl AsEngineRef, graph: &mut Graph) -> Result<()> {
        let engine_ref = engine.as_engine_ref();
        let engine = engine_ref.engine();
        let main_tls = (1..graph.libraries.len())
            .map(|index| graph.main_tls_imports(index))
            .collect::<Result<Vec<_>>>()?;
        let mut artifacts = lock(&self.artifacts);
        for (library, main_tls) in graph.libraries.iter_mut().skip(1).zip(main_tls) {
            let key = (
                library.code_hash,
                main_tls
                    .iter()
                    .map(|(index, offset)| (*index, *offset))
                    .collect(),
                engine.deterministic_id(),
                engine.artifact_format(),
            );
            let artifact = match artifacts.entry(key) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => entry.insert(Arc::new(
                    crate::artifact::PreparedArtifact::compile_main_tls(
                        engine,
                        library.initialization.clone(),
                        &main_tls,
                    )?,
                )),
            };
            library.prepared = Some(artifact.clone());
        }
        Ok(())
    }
    fn attach_cleanup(&self, scope: &Arc<wasixv2_host::LoaderCleanup>) {
        let mut scopes = lock(&self.cleanup);
        scopes.retain(|scope| scope.strong_count() != 0);
        if !scopes
            .iter()
            .any(|old| old.upgrade().is_some_and(|old| Arc::ptr_eq(&old, scope)))
        {
            scopes.push(Arc::downgrade(scope));
        }
    }
    fn submit_cleanup(
        &self,
        mut job: wasixv2_host::LoaderCleanupJob,
    ) -> std::result::Result<(), wasixv2_host::LoaderCleanupJob> {
        for scope in lock(&self.cleanup).iter().rev().filter_map(Weak::upgrade) {
            match scope.submit(job) {
                Ok(()) => return Ok(()),
                Err(returned) => job = returned,
            }
        }
        Err(job)
    }
    fn register_retirement<S: wasixv2_syscalls::LoaderControl + Send + Sync + 'static>(
        self: &Arc<Self>,
        scope: &Arc<wasixv2_host::LoaderCleanup>,
        syscalls: Arc<S>,
    ) -> Result<()> {
        let root = Arc::downgrade(self);
        let runner = Arc::downgrade(scope);
        scope
            .on_retire(
                &self.owner,
                Box::new(move || {
                    Box::pin(async move {
                        if let (Some(root), Some(scope)) = (root.upgrade(), runner.upgrade())
                            && let Err(error) = root.retire_common(&*syscalls, &scope).await
                        {
                            let _ = syscalls.loader_fatal(root.gate().unwrap());
                            tracing::error!(%error, "logical image retirement failed");
                        }
                    })
                }),
            )
            .map_err(|_| failure("runner already retired"))
    }
    async fn retire_common<S: wasixv2_syscalls::LoaderControl>(
        self: &Arc<Self>,
        syscalls: &S,
        scope: &Arc<wasixv2_host::LoaderCleanup>,
    ) -> Result<()> {
        let gate = self.gate()?;
        if self.retired.load(Ordering::Acquire)
            || syscalls
                .loader_members(gate)
                .ok_or_else(|| failure("logical retirement unsupported"))?
                != 0
        {
            return Ok(());
        }
        gate.wait_idle().await;
        if syscalls
            .loader_members(gate)
            .ok_or_else(|| failure("logical retirement unsupported"))?
            != 0
            || self.retired.swap(true, Ordering::AcqRel)
        {
            return Ok(());
        }
        let leases = {
            let mut state = lock(&self.state);
            for library in &mut Arc::make_mut(&mut state.graph).libraries {
                library.prepared = None;
            }
            lock(&self.artifacts).clear();
            self.forward_code.clear();
            let mut leases = std::mem::take(&mut state.leases);
            if let Some(work) = state.work.take() {
                leases.extend(std::mem::take(&mut *lock(&work.leases)));
            }
            leases
        };
        LeaseLedger {
            root: self.clone(),
            cleanup: scope.clone(),
            leases,
            pending: None,
        }
        .release()
        .await;
        Ok(())
    }
    fn gate(&self) -> Result<&Arc<LoaderGate>> {
        self.gate.get().ok_or_else(|| failure("image has no mm"))
    }
    fn graph(&self) -> Arc<Graph> {
        lock(&self.state).graph.clone()
    }
    fn admitted_store(&self, gate: &Arc<LoaderGate>) -> Result<()> {
        // Exec's Binding holds the original admission through host preparation. A
        // closer must wait for it; attempting a second claim here would reject an
        // already admitted peer precisely while that closer waits for it to finish.
        if !self
            .gate
            .get()
            .is_some_and(|bound| Arc::ptr_eq(bound, gate))
        {
            return Err(failure("store has no admitted image attachment"));
        }
        Ok(())
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let mut state = lock(&self.state);
        let mut leases = std::mem::take(&mut state.leases);
        if let Some(work) = state.work.take() {
            leases.extend(std::mem::take(&mut *lock(&work.leases)));
        }
        drop(state);
        if leases.is_empty() {
            return;
        }
        let owner = self.owner.clone();
        let job: wasixv2_host::LoaderCleanupJob = Box::new(move || {
            Box::pin(async move {
                for lease in leases {
                    lease.release().await;
                }
                drop(owner);
            })
        });
        if let Err(job) = self.submit_cleanup(job)
            && let Some(host) = self.host.get()
        {
            // Trusted hosts retaining a specialized ABI past every runner still need
            // the existing executor context. This unmanaged lifetime is reported separately.
            host.executor().spawn_local(job());
        }
    }
}
fn align(value: u32, alignment: u32) -> Result<u32> {
    value
        .checked_add(alignment - 1)
        .map(|v| v & !(alignment - 1))
        .ok_or_else(|| failure("layout overflow"))
}
struct Image<S> {
    root: Arc<Root>,
    base: Arc<dyn Abi<S>>,
}
impl<S: Syscalls + Control + Send + Sync + 'static> Abi<S> for Image<S> {
    fn attach_cleanup(&self, scope: &Arc<wasixv2_host::LoaderCleanup>) {
        self.root.attach_cleanup(scope);
        self.base.attach_cleanup(scope);
    }

    fn name(&self) -> &'static str {
        "wasixv2-dylink-image"
    }
    fn matches(&self, _: &Module) -> bool {
        true
    }
    fn matches_image(&self, _: &[u8]) -> bool {
        true
    }
    fn claim_image(&self, gate: &Arc<LoaderGate>) -> std::result::Result<LoaderAdmission, Errno> {
        if self.root.retired.load(Ordering::Acquire) {
            return Err(Errno::ESTALE);
        }
        if self.root.error.get().is_some() {
            return Err(Errno::ENOEXEC);
        }
        if self
            .root
            .gate
            .get()
            .is_some_and(|old| !Arc::ptr_eq(old, gate))
        {
            return Err(Errno::ESTALE);
        }
        let claim = gate.claim_image(&self.root.owner)?;
        if let Some(old) = self.root.gate.get()
            && !Arc::ptr_eq(old, gate)
        {
            return Err(Errno::ESTALE);
        }
        let _ = self.root.gate.set(gate.clone());
        Ok(claim)
    }
    fn imports(
        &self,
        store: &mut Store,
        env: &FunctionEnv<ExecEnv<S>>,
        module: &Module,
    ) -> Imports {
        let mut imports = self.base.imports(store, env, module);
        let data = env.as_ref(store);
        let cleanup = data.loader_cleanup().cloned();
        let task = data.task();
        let syscalls = data.syscalls().clone();
        let host = data.host().clone();
        let setup = (|| -> Result<()> {
            if let Some(error) = self.root.error.get() {
                return Err(failure(error));
            }
            if self.root.retired.load(Ordering::Acquire) {
                return Err(failure("image retired"));
            }
            if !syscalls.loader_fatal_supported() {
                return Err(failure("image-wide fatal failure control is unavailable"));
            }
            let gate = syscalls
                .loader_gate(task)
                .ok_or_else(|| failure("dynamic image requires mm control"))?;
            self.root.admitted_store(&gate)?;
            if syscalls.loader_members(&gate).is_none() {
                return Err(failure("logical retirement control unavailable"));
            }
            let scope = cleanup
                .clone()
                .ok_or_else(|| failure("dynamic image requires managed runner"))?;
            self.root.register_retirement(&scope, syscalls.clone())?;

            let _ = self.root.host.set(host);
            let graph = self.root.graph();
            let main = &graph.libraries[0];
            for import in module.imports() {
                match import.ty() {
                    wasmer::ExternType::Global(ty) => {
                        let value = match import.name() {
                            "__memory_base" => main.base,
                            "__table_base" => main.table,
                            "__stack_pointer" => self.root.stack,
                            name if matches!(import.module(), "GOT.func" | "GOT.mem") => {
                                let end = main
                                    .base
                                    .checked_add(main.metadata.memory_info().memory_size)
                                    .ok_or_else(|| failure("main data overflow"))?;
                                let Some(value) =
                                    self.root.policy.main_global(name, self.root.stack, end)
                                else {
                                    continue;
                                };
                                value
                            }
                            _ => {
                                return Err(failure(format!(
                                    "unsupported dynamic main global {}",
                                    import.name()
                                )));
                            }
                        };
                        if ty.ty != Type::I32 {
                            return Err(failure("non-i32 main infrastructure"));
                        }
                        let global = if ty.mutability == Mutability::Var {
                            Global::new_mut(store, Value::I32(value as i32))
                        } else {
                            Global::new(store, Value::I32(value as i32))
                        };
                        imports.define(import.module(), import.name(), global);
                    }
                    wasmer::ExternType::Tag(ty) => {
                        if let Some(params) =
                            self.root.policy.main_tag(import.module(), import.name())
                        {
                            if ty.params.as_ref() != params {
                                return Err(failure("invalid infrastructure tag signature"));
                            }
                            if imports.get_export(import.module(), import.name()).is_none() {
                                imports.define(
                                    import.module(),
                                    import.name(),
                                    wasmer::Tag::new(store, params),
                                );
                            }
                        }
                    }
                    wasmer::ExternType::Table(ty) => {
                        let mut ty = *ty;
                        ty.minimum = ty.minimum.max(graph.cursor);
                        imports.define(
                            import.module(),
                            import.name(),
                            Table::new(store, ty, Value::FuncRef(None))?,
                        );
                    }
                    wasmer::ExternType::Memory(ty)
                        if !self.root.initialized.load(Ordering::Acquire) =>
                    {
                        if !ty.shared {
                            return Err(failure("dynamic memory must be shared"));
                        }
                        let end = main
                            .base
                            .checked_add(main.metadata.memory_info().memory_size)
                            .ok_or_else(|| failure("main data overflow"))?;
                        let mut ty = *ty;
                        ty.minimum = ty.minimum.max(wasmer::Pages(end.div_ceil(65536)));
                        if self.root.policy.memory_limit() < 1 << 32 {
                            let limit = wasmer::Pages(
                                u32::try_from(
                                    self.root.policy.memory_limit()
                                        / wasixv2_syscalls::WASM_PAGE_SIZE,
                                )
                                .map_err(failure)?,
                            );
                            ty.maximum =
                                Some(ty.maximum.map_or(limit, |maximum| maximum.min(limit)));
                        }
                        imports.define(
                            import.module(),
                            import.name(),
                            Memory::new(store, ty).map_err(failure)?,
                        );
                    }
                    _ => {}
                }
            }
            Ok(())
        })();
        if setup.is_err() {
            return Imports::new();
        }
        imports
    }
    fn prepare<'a>(
        &'a self,
        cx: &'a wasixv2_exec::Preparation<'a, S>,
    ) -> LocalFuture<'a, startup::PreparedResult<S>> {
        Box::pin(self.prepare_startup(cx))
    }
    fn bind(
        &self,
        instance: &Instance,
        store: &mut Store,
        task: TaskId,
        role: Role,
    ) -> std::result::Result<Arc<dyn AbiInstance<S>>, ExecError> {
        self.bind_store(instance, store, task, role)
            .map(|image| image as Arc<dyn AbiInstance<S>>)
    }
    fn image_layout(
        &self,
        _: &Instance,
        memory: &Memory,
        store: &mut Store,
        _: bool,
    ) -> Option<wasixv2_syscalls::MemoryLayout> {
        if !self.root.policy.mapped_memory() {
            return None;
        }
        let graph = self.root.graph();
        let main = graph.libraries.first()?;
        let end = u64::from(main.base) + u64::from(main.metadata.memory_info().memory_size);
        Some(wasixv2_syscalls::MemoryLayout {
            image_end: end,
            data: u64::from(main.base)..end,
            brk: end.next_multiple_of(16),
            maximum: memory
                .ty(store)
                .maximum
                .map_or(1 << 32, |p| u64::from(p.0) * 65536),
            reserved: Vec::new(),
        })
    }
    fn main_stack(&self, _: &Instance, _: &mut Store) -> Range<u32> {
        0..self.root.stack
    }
    fn guardable(&self, _: &Instance, _: &mut Store) -> bool {
        // Match exec::memory::protect_low: JS and hosts without page protection
        // cannot claim a guard during pre-instantiation bootstrap.
        cfg!(all(unix, feature = "sys"))
    }
    fn low_guard(&self) -> bool {
        true
    }
}
impl<S: Syscalls + Control + Send + Sync + 'static> Image<S> {
    fn bind_store(
        &self,
        instance: &Instance,
        store: &mut Store,
        task: TaskId,
        role: Role,
    ) -> std::result::Result<Arc<StoreImage<S>>, ExecError> {
        if let Some(error) = self.root.error.get() {
            return Err(ExecError::Load(wasixv2_exec::LoadError::other(failure(
                error,
            ))));
        }
        let inner = self.base.bind(instance, store, task, role)?;
        let memory = instance
            .exports
            .get_memory("__wasix_dylink_memory_0")?
            .clone();
        let table = instance
            .exports
            .get_table("__indirect_function_table")
            .or_else(|_| instance.exports.get_table("__wasix_dylink_table_0"))?
            .clone();
        Ok(Arc::new(StoreImage {
            root: self.root.clone(),
            inner,
            role,
            cleanup: OnceLock::new(),
            startup: Mutex::new(None),
            retired: Arc::new(AtomicBool::new(false)),
            servicing: AtomicBool::new(false),
            local: Arc::new(Mutex::new(Local {
                forward_code: self.root.forward_code.clone(),
                instances: vec![BoundModule::new(instance)],
                forwarders: Vec::new(),
                tls: vec![0],
                memory,
                table,
                stage: None,
                leases: Vec::new(),
                generation: 0,
                lazy_targets: BTreeMap::new(),
            })),
            _type: std::marker::PhantomData,
        }))
    }
}
struct Local {
    forward_code: Arc<forward::Cache>,
    instances: Vec<BoundModule>,
    forwarders: Vec<BoundModule>,
    tls: Vec<u32>,
    memory: Memory,
    table: Table,
    stage: Option<LocalStage>,
    leases: Vec<Box<dyn LoaderReservation>>,
    generation: u64,
    lazy_targets: BTreeMap<u32, Option<(usize, u32)>>,
}
impl Local {
    #[cfg(all(test, feature = "engine-tests"))]
    fn symbol(
        &self,
        store: &mut StoreMut<'_>,
        graph: &Graph,
        handle: usize,
        name: &str,
    ) -> Result<u32> {
        let mut tls = self.tls.clone();
        if let Some(base) = tls.first_mut() {
            // musl ldso/dynlink.c::do_dlsym selects the calling thread's TLS.
            // pthread/CLONE_VM entry can change main's base after main_prepare.
            *base = self.instances[0]
                .exports
                .get_global("__tls_base")
                .ok()
                .and_then(|global| global.get(&mut *store).i32())
                .unwrap_or(0) as u32;
        }
        graph.symbol(handle, name, &tls)
    }

    fn symbol_in_scope(
        &self,
        store: &mut StoreMut<'_>,
        graph: &Graph,
        scope: &[usize],
        name: &str,
    ) -> Result<u32> {
        let mut tls = self.tls.clone();
        if let Some(base) = tls.first_mut() {
            *base = self.instances[0]
                .exports
                .get_global("__tls_base")
                .ok()
                .and_then(|global| global.get(&mut *store).i32())
                .unwrap_or(0) as u32;
        }
        graph.symbol_in_scope(scope, name, &tls)
    }

    #[cfg(all(test, feature = "engine-tests"))]
    fn stage_modules(
        &mut self,
        store: &mut StoreMut<'_>,
        base_imports: &Imports,
        work: &Work,
        tls: Vec<u32>,
        leases: Vec<Box<dyn LoaderReservation>>,
        task: TaskId,
    ) -> Result<()> {
        self.stage_modules_with_hooks(store, base_imports, work, tls, leases, task, None)
    }

    #[allow(clippy::too_many_arguments)]
    fn stage_modules_with_hooks(
        &mut self,
        store: &mut StoreMut<'_>,
        base_imports: &Imports,
        work: &Work,
        tls: Vec<u32>,
        leases: Vec<Box<dyn LoaderReservation>>,
        task: TaskId,
        hooks: Option<&wasixv2_exec::InstanceHooks>,
    ) -> Result<()> {
        LocalStage::resume(&mut self.stage, tls.clone(), leases);
        let local = self;
        let graph = work.graph()?;
        local
            .instances
            .try_reserve(graph.libraries.len() - work.previous)
            .map_err(failure)?;
        local
            .forwarders
            .try_reserve(graph.libraries.len() - work.previous)
            .map_err(failure)?;
        let count = local.stage.as_ref().unwrap().leases.len();
        local.leases.try_reserve(count).map_err(failure)?;
        for index in graph
            .initialization
            .iter()
            .copied()
            .filter(|index| *index >= work.previous)
        {
            let lib = &graph.libraries[index];
            let mut imports = base_imports.clone();
            let mut pending = Vec::new();
            let main_tls = graph.main_tls_imports(index)?;
            let mut global_index = 0u32;
            for (ordinal, import) in lib.metadata.imports().iter().enumerate() {
                let tls_alias = if matches!(import.ty, ExternType::Global(_)) {
                    let alias = main_tls.contains_key(&global_index);
                    global_index += 1;
                    alias
                } else {
                    false
                };
                let fixup = lib
                    .fixups
                    .iter()
                    .find(|f| f.module == index && f.import == ordinal);
                let value = match &import.ty {
                    ExternType::Memory(_) => Some(Extern::Memory(local.memory.clone())),
                    ExternType::Table(_) => Some(Extern::Table(local.table.clone())),
                    ExternType::Global(_) if tls_alias => {
                        // The normalized code adds the frozen symbol offset. Sharing
                        // this store's live base follows libc bootstrap without a
                        // callback or writes to a parked peer's initialized pointers.
                        Some(Extern::Global(
                            local.instances[0]
                                .exports
                                .get_global("__tls_base")
                                .map_err(failure)?
                                .clone(),
                        ))
                    }
                    ExternType::Global(_) if import.name == "__stack_pointer" => {
                        Some(Extern::Global(
                            local.instances[0]
                                .exports
                                .get_global("__stack_pointer")
                                .map_err(failure)?
                                .clone(),
                        ))
                    }
                    ExternType::Global(ty) => {
                        let value = match import.name.as_str() {
                            "__memory_base" => lib.base,
                            "__table_base" => lib.table,
                            "__tls_base" => tls[index],
                            _ => graph.target_address(
                                &fixup.ok_or_else(|| failure("missing GOT fixup"))?.target,
                                &tls,
                            )?,
                        };
                        let value = match ty.content_type {
                            wasmparser::ValType::I32 => Value::I32(value as i32),
                            wasmparser::ValType::I64 => Value::I64(i64::from(value)),
                            _ => return Err(failure("unsupported GOT type")),
                        };
                        Some(Extern::Global(if ty.mutable {
                            Global::new_mut(store, value)
                        } else {
                            Global::new(store, value)
                        }))
                    }
                    ExternType::Function(signature)
                        if fixup.is_some_and(|f| f.kind != FixupKind::Host) =>
                    {
                        if fixup.is_some_and(|f| f.target == BindingTarget::Deferred) {
                            let lazy = lib
                                .lazy
                                .get(&ordinal)
                                .ok_or_else(|| failure("missing deferred function slot"))?;
                            pending.push((ordinal, signature.clone(), lazy.slot));
                            continue;
                        }
                        let BindingTarget::Function { module, index, .. } = &fixup.unwrap().target
                        else {
                            return Err(failure("missing weak direct function is unsupported"));
                        };
                        let provider = if *module < work.previous {
                            local.instances.get(*module)
                        } else {
                            local
                                .stage
                                .as_ref()
                                .and_then(|s| s.modules.get(module))
                                .map(StagedExports::instance)
                        };
                        if let Some(provider) = provider {
                            Some(Extern::Function(engine_function(provider, *index)?))
                        } else {
                            let slot = graph.target_address(&fixup.unwrap().target, &tls)?;
                            pending.push((ordinal, signature.clone(), slot));
                            None
                        }
                    }
                    ExternType::Tag(_) => {
                        let mut found = None;
                        for provider in &local.instances {
                            if let Ok(export) = provider.exports.get::<Extern>(&import.name)
                                && matches!(export, Extern::Tag(_))
                            {
                                found = Some(export.clone());
                                break;
                            }
                        }
                        Some(found.ok_or_else(|| failure("missing main exception tag"))?)
                    }
                    _ => None,
                };
                if let Some(value) = value {
                    imports.define(&import.module, &import.name, value);
                }
            }
            if !pending.is_empty() {
                let targets = pending
                    .iter()
                    .map(|(_, ty, slot)| (ty.clone(), *slot))
                    .collect::<Vec<_>>();
                let forwarder = forward::instantiate(
                    &local.forward_code,
                    store,
                    &local.table,
                    base_imports,
                    &targets,
                )?;
                for (number, (ordinal, _, _)) in pending.iter().enumerate() {
                    let import = &lib.metadata.imports()[*ordinal];
                    imports.define(
                        &import.module,
                        &import.name,
                        forwarder
                            .exports
                            .get_function(&forward::name(number))
                            .map_err(failure)?
                            .clone(),
                    );
                }
                local
                    .stage
                    .as_mut()
                    .unwrap()
                    .forwarders
                    .push(BoundModule::new(&forwarder));
            }
            let fresh_memory = if task == work.first {
                u64::from(lib.base)
                    ..u64::from(lib.base) + u64::from(lib.metadata.memory_info().memory_size)
            } else {
                u64::from(tls[index])..u64::from(tls[index]) + u64::from(lib.tls_size)
            };
            let fresh_table = u64::from(lib.table)..u64::from(graph.cursor);
            let prepared = StagedExports::prepare_artifact_with_hooks(
                store,
                lib.prepared
                    .as_deref()
                    .ok_or_else(|| failure("missing prepared module"))?,
                &mut imports,
                local.memory.clone(),
                local.table.clone(),
                std::slice::from_ref(&fresh_memory),
                std::slice::from_ref(&fresh_table),
                if task == work.first {
                    None
                } else {
                    Some(tls[index])
                },
                hooks,
            )?;
            local
                .stage
                .as_mut()
                .unwrap()
                .modules
                .insert(index, prepared);
            let prepared = local
                .stage
                .as_mut()
                .unwrap()
                .modules
                .get_mut(&index)
                .unwrap();
            for (function, slot) in &lib.slots {
                prepared.bind_slot(store, *slot, *function, std::slice::from_ref(&fresh_table))?;
            }
        }
        // All definitions now exist. Element/GOT slots must contain actual
        // definition funcrefs, including imported/re-exported aliases, not thunks.
        let mut aliases = BTreeMap::new();
        for index in work.previous..graph.libraries.len() {
            let mut functions = BTreeMap::new();
            let mut function = 0;
            for (ordinal, import) in graph.libraries[index].metadata.imports().iter().enumerate() {
                if matches!(import.ty, ExternType::Function(_)) {
                    if graph.libraries[index].fixups.iter().any(|fixup| {
                        fixup.import == ordinal && fixup.target == BindingTarget::Deferred
                    }) {
                        function += 1;
                        continue;
                    }
                    let (module, target) = graph.function_definition(index, function)?;
                    let provider = if module < work.previous {
                        &local.instances[module]
                    } else {
                        local.stage.as_ref().unwrap().modules[&module].instance()
                    };
                    functions.insert(function, engine_function(provider, target)?);
                    function += 1;
                }
            }
            aliases.insert(index, functions);
        }
        for (index, functions) in aliases {
            local
                .stage
                .as_mut()
                .unwrap()
                .modules
                .get_mut(&index)
                .unwrap()
                .resolve_aliases(&functions);
        }
        local.stage_lazy(store, &graph, base_imports)?;
        // A LAZY module may own several trap instances, in addition to its
        // ordinary forwarder. Publication must not allocate after commit.
        local
            .forwarders
            .try_reserve(local.stage.as_ref().unwrap().forwarders.len())
            .map_err(failure)?;
        Ok(())
    }

    fn stage_lazy(
        &mut self,
        store: &mut StoreMut<'_>,
        graph: &Graph,
        imports: &Imports,
    ) -> Result<()> {
        let size = self.table.size(&*store);
        if graph.cursor > size {
            self.table
                .grow(store, graph.cursor - size, Value::FuncRef(None))?;
        }
        for lib in &graph.libraries {
            for (ordinal, lazy) in &lib.lazy {
                self.stage
                    .as_mut()
                    .unwrap()
                    .lazy_targets
                    .insert(lazy.slot, lazy.target);
                if self.lazy_targets.get(&lazy.slot) == Some(&lazy.target) {
                    continue;
                }
                let function = if let Some((module, index)) = lazy.target {
                    let provider = self
                        .instances
                        .get(module)
                        .or_else(|| {
                            self.stage
                                .as_ref()?
                                .modules
                                .get(&module)
                                .map(StagedExports::instance)
                        })
                        .ok_or_else(|| failure("missing deferred target instance"))?;
                    engine_function(provider, index)?
                } else {
                    let ExternType::Function(signature) = &lib.metadata.imports()[*ordinal].ty
                    else {
                        return Err(failure("deferred non-function import"));
                    };
                    let instance = forward::traps(
                        &self.forward_code,
                        store,
                        &self.table,
                        imports,
                        std::slice::from_ref(signature),
                    )?;
                    let function = instance
                        .exports
                        .get_function(&forward::name(0))
                        .map_err(failure)?
                        .clone();
                    self.stage
                        .as_mut()
                        .unwrap()
                        .forwarders
                        .push(BoundModule::new(&instance));
                    function
                };
                let before = self
                    .table
                    .get(store, lazy.slot)
                    .ok_or_else(|| failure("missing deferred slot"))?;
                self.stage.as_mut().unwrap().lazy_slots.push(LazySlot {
                    slot: lazy.slot,
                    before,
                    function,
                    applied: false,
                });
            }
        }
        Ok(())
    }
}
struct LeaseLedger {
    cleanup: Arc<wasixv2_host::LoaderCleanup>,
    root: Arc<Root>,
    leases: Vec<Box<dyn LoaderReservation>>,
    pending: Option<LocalFuture<'static, ()>>,
}
impl LeaseLedger {
    fn take_pending(&mut self) -> Option<LocalFuture<'static, ()>> {
        self.pending.take().or_else(|| {
            let leases = std::mem::take(&mut self.leases);
            (!leases.is_empty()).then(|| {
                Box::pin(async move {
                    for lease in leases {
                        lease.release().await;
                    }
                }) as LocalFuture<'static, ()>
            })
        })
    }
    async fn release(mut self) {
        self.pending = self.take_pending();
        if let Some(pending) = &mut self.pending {
            pending.await;
        }
        self.pending = None;
    }
}
impl Drop for LeaseLedger {
    fn drop(&mut self) {
        if let Some(pending) = self.take_pending() {
            let owner = self.root.owner.clone();
            self.cleanup.spawn(Box::pin(async move {
                pending.await;
                drop(owner);
            }));
        }
    }
}
struct LocalStage {
    modules: BTreeMap<usize, StagedExports>,
    forwarders: Vec<BoundModule>,
    tls: Vec<u32>,
    leases: Vec<Box<dyn LoaderReservation>>,
    lazy_targets: BTreeMap<u32, Option<(usize, u32)>>,
    lazy_slots: Vec<LazySlot>,
}
struct LazySlot {
    slot: u32,
    before: Value,
    function: wasmer::Function,
    applied: bool,
}
impl LocalStage {
    fn resume(stage: &mut Option<Self>, tls: Vec<u32>, leases: Vec<Box<dyn LoaderReservation>>) {
        let stage = stage.get_or_insert_with(|| Self {
            modules: BTreeMap::new(),
            forwarders: Vec::new(),
            tls: Vec::new(),
            leases: Vec::new(),
            lazy_targets: BTreeMap::new(),
            lazy_slots: Vec::new(),
        });
        stage.tls = tls;
        stage.leases.extend(leases);
    }
    fn apply(&mut self, store: &mut impl AsStoreMut, table: &Table) -> Result<()> {
        for module in self.modules.values_mut() {
            module.apply(store)?;
        }
        for slot in &mut self.lazy_slots {
            table.set(
                store,
                slot.slot,
                Value::FuncRef(Some(slot.function.clone())),
            )?;
            slot.applied = true;
        }
        Ok(())
    }
    fn rollback(&mut self, store: &mut impl AsStoreMut, table: &Table) -> Result<()> {
        for slot in self.lazy_slots.iter_mut().rev().filter(|slot| slot.applied) {
            table.set(store, slot.slot, slot.before.clone())?;
            slot.applied = false;
        }
        for module in self.modules.values_mut() {
            module.rollback(store)?;
        }
        Ok(())
    }
}
struct StoreImage<S: 'static> {
    startup: Mutex<Option<startup::Bound<S>>>,
    retired: Arc<AtomicBool>,
    cleanup: OnceLock<Arc<wasixv2_host::LoaderCleanup>>,
    root: Arc<Root>,
    inner: Arc<dyn AbiInstance<S>>,
    role: Role,
    servicing: AtomicBool,
    local: Arc<Mutex<Local>>,
    _type: std::marker::PhantomData<S>,
}
impl<S: Syscalls + Control + Send + Sync + 'static> StoreImage<S> {
    fn ensure_live(&self) -> Result<()> {
        if self.retired.load(Ordering::Acquire) || self.root.retired.load(Ordering::Acquire) {
            Err(failure("store retired"))
        } else {
            Ok(())
        }
    }

    /// A transaction claims its entire constructor batch before guest entry.
    /// Resident reopens never replay it, including after a trap or cancellation,
    /// matching v1's consumed new_modules list. No loader lock spans a guest call.
    async fn call_constructors(&self, cx: &Context<'_, S>, slots: &[u32]) -> Result<()> {
        let functions = cx
            .with(|_, store| {
                let local = lock(&self.local);
                slots
                    .iter()
                    .filter(|slot| **slot != 0)
                    .map(|slot| match local.table.get(store, *slot) {
                        Some(Value::FuncRef(Some(function))) => Ok(function),
                        _ => Err(failure("constructor slot is not a function")),
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .await?;
        for function in functions {
            match cx {
                Context::Import(env) => {
                    wasixv2_exec::call_guest(env, &function, &[]).await?;
                }
                Context::Entry(entry) => {
                    entry.call_once(&function, &[]).await?;
                }
            }
        }
        Ok(())
    }

    fn main_prepare(&self, store: &mut StoreMut<'_>, graph: &Graph) -> Result<()> {
        let main = &graph.libraries[0];
        let mut local = lock(&self.local);
        let instance = &local.instances[0];
        let inputs = engine_globals(instance, store)?;
        let init = &main.initialization;
        if self.role == Role::Process {
            if self.root.initialized.load(Ordering::Acquire) {
                return Err(failure("second process entry for image"));
            }
            let end = u64::from(main.base) + u64::from(main.metadata.memory_info().memory_size);
            let effects = init
                .effects(
                    inputs.clone(),
                    std::slice::from_ref(&(u64::from(main.base)..end)),
                    None,
                )
                .map_err(failure)?;
            for write in effects.writes {
                local
                    .memory
                    .view(&*store)
                    .write(u64::from(write.address), &write.bytes)
                    .map_err(failure)?;
            }
            apply_globals(instance, store, &effects.globals)?;
        } else {
            let effects = init.peer_globals(inputs.clone()).map_err(failure)?;
            apply_globals(instance, store, &effects.globals)?;
        }
        let table = local.table.clone();
        let size = table.size(&*store);
        if graph.cursor > size {
            table.grow(store, graph.cursor - size, Value::FuncRef(None))?;
        }
        let fresh = u64::from(main.table)
            ..u64::from(main.table) + u64::from(main.metadata.memory_info().table_size);
        for element in init
            .table_effects(&inputs, std::slice::from_ref(&fresh))
            .map_err(failure)?
        {
            for (offset, function) in element.functions.iter().enumerate() {
                if let Some(index) = function {
                    table.set(
                        store,
                        element.index + offset as u32,
                        Value::FuncRef(Some(engine_function(instance, *index)?)),
                    )?;
                }
            }
        }
        for (index, slot) in &main.slots {
            let function = engine_function(instance, *index)?;
            // This table is fresh and both assignments use the validated source function
            // index. Table::get may materialize a different Rust handle for the same wasm
            // funcref, so handle Eq is not an identity check after a table round trip.
            table.set(store, *slot, Value::FuncRef(Some(function)))?;
        }
        local.tls[0] = instance
            .exports
            .get_global("__tls_base")
            .ok()
            .and_then(|g| g.get(&mut *store).i32())
            .unwrap_or(0) as u32;
        self.root.initialized.store(true, Ordering::Release);
        Ok(())
    }
    async fn prepare_modules(
        &self,
        cx: &Context<'_, S>,
        caller: &Caller<'_>,
        work: &Work,
    ) -> Result<()> {
        let graph = work.graph()?;
        let (syscalls, current_tls) = cx
            .with(|data, _| (data.syscalls().clone(), lock(&self.local).tls.clone()))
            .await;
        let mut tls = current_tls;
        tls.resize(graph.libraries.len(), 0);
        // Keep each acquired lease in the rollback stage before the next await or
        // fallible preparation step. Prepare failure must not detach TLS cleanup
        // and then acknowledge Rollback while that cleanup is still pending.
        LocalStage::resume(&mut lock(&self.local).stage, tls.clone(), Vec::new());
        for (index, tls_base) in tls.iter_mut().enumerate().skip(work.previous) {
            let lib = &graph.libraries[index];
            if caller.task() == work.first {
                if let Some(symbol) = lib.metadata.exports().get("__tls_base") {
                    let globals =
                        initial_globals(&lib.source, infrastructure(lib, self.root.stack, 0))?;
                    let end =
                        u64::from(lib.base) + u64::from(lib.metadata.memory_info().memory_size);
                    let effects = lib
                        .initialization
                        .effects(
                            globals.clone(),
                            std::slice::from_ref(&(u64::from(lib.base)..end)),
                            None,
                        )
                        .map_err(failure)?;
                    *tls_base = effects
                        .globals
                        .get(&symbol.index)
                        .copied()
                        .or_else(|| globals.get(&symbol.index).copied())
                        .unwrap_or(0) as u32;
                }
            } else if lib.tls_size > 0 {
                let lease = syscalls
                    .loader_reserve(caller, lib.tls_size, lib.tls_align)
                    .await
                    .map_err(failure)?;
                *tls_base = lease.address();
                lock(&self.local).stage.as_mut().unwrap().leases.push(lease);
            }
        }
        cx.with(|data, store| {
            self.stage_modules(
                store,
                data.abi_imports(),
                work,
                tls,
                Vec::new(),
                caller.task(),
                data.instance_hooks(),
            )
        })
        .await
    }
    #[allow(clippy::too_many_arguments)]
    fn stage_modules(
        &self,
        store: &mut StoreMut<'_>,
        base_imports: &Imports,
        work: &Work,
        tls: Vec<u32>,
        leases: Vec<Box<dyn LoaderReservation>>,
        task: TaskId,
        hooks: Option<&wasixv2_exec::InstanceHooks>,
    ) -> Result<()> {
        lock(&self.local).stage_modules_with_hooks(
            store,
            base_imports,
            work,
            tls,
            leases,
            task,
            hooks,
        )
    }
    async fn execute_command(
        &self,
        cx: &Context<'_, S>,
        task: TaskId,
        command: wasixv2_syscalls::LoaderCommand,
    ) -> Result<()> {
        let work = lock(&self.root.state).work.clone();
        match command.phase {
            LoaderPhase::Park => {}
            LoaderPhase::Prepare => {
                let work = work
                    .as_deref()
                    .ok_or_else(|| failure("missing load work"))?;
                match cx {
                    Context::Import(env) => {
                        let memory = wasixv2_exec::GuestMemory::new(env);
                        self.prepare_modules(cx, &Caller::new(task, &memory), work)
                            .await?;
                    }
                    Context::Entry(entry) => {
                        let memory = entry.memory();
                        self.prepare_modules(cx, &Caller::new(task, &memory), work)
                            .await?;
                    }
                }
            }
            LoaderPhase::Apply => {
                cx.with(|_, store| {
                    let mut local = lock(&self.local);
                    let table = local.table.clone();
                    local
                        .stage
                        .as_mut()
                        .ok_or_else(|| failure("missing prepared store"))?
                        .apply(store, &table)?;
                    Ok::<(), RuntimeError>(())
                })
                .await?;
            }
            LoaderPhase::Publish => {
                let mut local = lock(&self.local);
                if let Some(mut staged) = local.stage.take() {
                    local.leases.extend(std::mem::take(&mut staged.leases));
                    for module in staged.modules.into_values() {
                        local.instances.push(module.publish()?);
                    }
                    local.forwarders.extend(staged.forwarders);
                    local.tls = staged.tls;
                    local.lazy_targets = staged.lazy_targets;
                    local.leases.extend(staged.leases);
                }
                local.generation = self.root.gate()?.generation() + 1;
            }
            LoaderPhase::Rollback => {
                let stage = cx
                    .with(|_, store| {
                        let mut local = lock(&self.local);
                        let table = local.table.clone();
                        if let Some(staged) = &mut local.stage {
                            staged.rollback(store, &table)?;
                        }
                        Ok::<_, RuntimeError>(local.stage.take())
                    })
                    .await?;
                if let Some(stage) = stage {
                    LeaseLedger {
                        cleanup: self
                            .cleanup
                            .get()
                            .ok_or_else(|| failure("missing runner cleanup scope"))?
                            .clone(),
                        root: self.root.clone(),
                        leases: stage.leases,
                        pending: None,
                    }
                    .release()
                    .await;
                }
            }
        }
        Ok(())
    }
    async fn command(
        &self,
        cx: &Context<'_, S>,
        task: TaskId,
        command: wasixv2_syscalls::LoaderCommand,
    ) -> Result<()> {
        if let Err(error) = self.execute_command(cx, task, command).await {
            if matches!(command.phase, LoaderPhase::Prepare | LoaderPhase::Apply) {
                if let Some(work) = lock(&self.root.state).work.as_ref() {
                    let mut saved = lock(&work.error);
                    if saved.is_none() {
                        *saved = Some(error.to_string());
                    }
                }
            } else {
                let syscalls = cx.with(|data, _| data.syscalls().clone()).await;
                let _ = syscalls.loader_fatal(self.root.gate()?);
                return Err(error);
            }
        }
        self.root
            .gate()?
            .acknowledge(task, command)
            .map_err(failure)
    }
}
fn engine_globals(
    instance: &BoundModule,
    store: &mut impl AsStoreMut,
) -> Result<BTreeMap<u32, u64>> {
    let mut globals = BTreeMap::new();
    for (name, export) in instance.exports.iter() {
        if let Some(index) = name.strip_prefix("__wasix_dylink_global_") {
            let Extern::Global(global) = export else {
                return Err(failure("private global mismatch"));
            };
            let value = match global.get(&mut *store) {
                Value::I32(v) => u64::from(v as u32),
                Value::I64(v) => v as u64,
                _ => return Err(failure("non-scalar initializer global")),
            };
            globals.insert(index.parse().map_err(failure)?, value);
        }
    }
    Ok(globals)
}
fn apply_globals(
    instance: &BoundModule,
    store: &mut impl AsStoreMut,
    globals: &BTreeMap<u32, u64>,
) -> Result<()> {
    for (index, value) in globals {
        let global = instance
            .exports
            .get_global(&format!("__wasix_dylink_global_{index}"))
            .map_err(failure)?;
        let value = match global.get(&mut *store) {
            Value::I32(_) => Value::I32(*value as i32),
            Value::I64(_) => Value::I64(*value as i64),
            _ => return Err(failure("non-scalar initializer result")),
        };
        global.set(store, value)?;
    }
    Ok(())
}
fn engine_function(instance: &BoundModule, index: u32) -> Result<wasmer::Function> {
    instance
        .exports
        .get_function(&format!("__wasix_dylink_fn_{index}"))
        .cloned()
        .map_err(failure)
}
async fn retire_local(
    root: Arc<Root>,
    local: Arc<Mutex<Local>>,
    cleanup: Arc<wasixv2_host::LoaderCleanup>,
) {
    let leases = {
        let mut local = lock(&local);
        let mut leases = std::mem::take(&mut local.leases);
        if let Some(stage) = local.stage.take() {
            leases.extend(stage.leases);
        }
        leases
    };
    LeaseLedger {
        root,
        cleanup,
        leases,
        pending: None,
    }
    .release()
    .await;
}
impl<S: Syscalls + Control + Send + Sync + 'static> AbiInstance<S> for StoreImage<S> {
    fn function_table(&self) -> Option<&Table> {
        self.inner.function_table()
    }

    fn retire<'a>(&'a self, entry: &'a Entry<'a, S>) -> LocalFuture<'a, Result<()>> {
        Box::pin(async move {
            self.retired.store(true, Ordering::Release);
            if let Some(scope) = self.cleanup.get() {
                retire_local(self.root.clone(), self.local.clone(), scope.clone()).await;
                // exec may already have replaced this task's mm. Retire only the
                // original gate when its last user left; fault release is checked
                // again by the weak final-runner callback after release_task.
                let syscalls = entry.with_store(|data, _| data.syscalls().clone()).await;
                self.root.retire_common(&*syscalls, scope).await?;
            }
            self.inner.retire(entry).await
        })
    }
    fn prepare_entry<'a>(&'a self, entry: &'a Entry<'a, S>) -> LocalFuture<'a, Result<()>> {
        Box::pin(async move {
            let scope = entry
                .with_store(|data, _| data.loader_cleanup().cloned())
                .await
                .ok_or_else(|| failure("dynamic image requires a managed runner"))?;
            self.ensure_live()?;
            if self.cleanup.set(scope.clone()).is_ok() {
                let root = Arc::downgrade(&self.root);
                let local = Arc::downgrade(&self.local);
                let runner = Arc::downgrade(&scope);
                let retired = self.retired.clone();
                scope
                    .on_retire(
                        &self.root.owner,
                        Box::new(move || {
                            Box::pin(async move {
                                retired.store(true, Ordering::Release);
                                if let (Some(root), Some(local), Some(scope)) =
                                    (root.upgrade(), local.upgrade(), runner.upgrade())
                                {
                                    retire_local(root, local, scope).await;
                                }
                            })
                        }),
                    )
                    .map_err(|_| failure("runner already retired"))?;
            }

            let startup = lock(&self.startup).take();
            if let Some(startup) = startup {
                self.finish_startup(entry, startup).await?;
                return self.inner.prepare_entry(entry).await;
            }
            let graph = self.root.graph();
            entry
                .with_store(|_, store| self.main_prepare(store, &graph))
                .await?;
            if graph.libraries.len() > 1 {
                let (syscalls, task) = entry
                    .with_store(|data, _| (data.syscalls().clone(), data.task()))
                    .await;
                let memory = entry.memory();
                let caller = Caller::new(task, &memory);
                let mut tls = lock(&self.local).tls.clone();
                tls.resize(graph.libraries.len(), 0);
                let mut ledger = LeaseLedger {
                    cleanup: self
                        .cleanup
                        .get()
                        .ok_or_else(|| failure("missing runner cleanup scope"))?
                        .clone(),
                    root: self.root.clone(),
                    leases: Vec::new(),
                    pending: None,
                };
                for (index, lib) in graph.libraries.iter().enumerate().skip(1) {
                    if lib.tls_size > 0 {
                        let lease = syscalls
                            .loader_reserve(&caller, lib.tls_size, lib.tls_align)
                            .await
                            .map_err(failure)?;
                        tls[index] = lease.address();
                        ledger.leases.push(lease);
                    }
                }
                let work = Work {
                    publication: Mutex::new(Some(Publication {
                        graph: graph.clone(),
                        references: BTreeMap::new(),
                        ctors: VecDeque::new(),
                    })),
                    previous: 1,
                    first: TaskId(0),
                    leases: Mutex::new(Vec::new()),
                    committed: AtomicBool::new(true),
                    error: Mutex::new(None),
                };
                entry
                    .with_store(|data, store| {
                        self.stage_modules(
                            store,
                            data.abi_imports(),
                            &work,
                            tls,
                            std::mem::take(&mut ledger.leases),
                            task,
                            data.instance_hooks(),
                        )
                    })
                    .await?;
                entry
                    .with_store(|_, store| {
                        let mut local = lock(&self.local);
                        let table = local.table.clone();
                        local
                            .stage
                            .as_mut()
                            .ok_or_else(|| failure("missing entry stage"))?
                            .apply(store, &table)?;
                        let mut stage = local.stage.take().unwrap();
                        local.leases.extend(std::mem::take(&mut stage.leases));
                        for module in stage.modules.into_values() {
                            local.instances.push(module.publish()?);
                        }
                        local.forwarders.extend(stage.forwarders);
                        local.tls = stage.tls;
                        local.lazy_targets = stage.lazy_targets;
                        local.leases.extend(stage.leases);
                        local.generation = self.root.gate()?.generation();
                        Ok::<(), RuntimeError>(())
                    })
                    .await?;
            }
            self.inner.prepare_entry(entry).await
        })
    }
    fn loader_work<'a>(
        &'a self,
        env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
    ) -> LocalFuture<'a, Result<()>> {
        Box::pin(async move { self.service(&Context::Import(env)).await })
    }
    fn cooperative_loader(&self) -> bool {
        true
    }
    fn loader_work_busy(&self) -> bool {
        self.servicing.load(Ordering::Acquire)
    }
    fn loader_work_outside<'a>(&'a self, entry: &'a Entry<'a, S>) -> LocalFuture<'a, Result<()>> {
        Box::pin(async move { self.service(&Context::Entry(entry)).await })
    }
    fn dl_open<'a>(
        &'a self,
        env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
        caller: &'a Caller<'_>,
        file: Cstr,
        flags: DlFlags,
        search: Cstr,
        caller_addr: Uptr,
        handle: DlHandlePtr,
        err: UserSliceMut,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(self.open(
            env,
            caller,
            file,
            flags,
            search,
            caller_addr,
            handle,
            err,
            None,
        ))
    }
    fn dl_open_with_unwind<'a>(
        &'a self,
        env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
        caller: &'a Caller<'_>,
        file: Cstr,
        flags: DlFlags,
        search: Cstr,
        caller_addr: Uptr,
        handle: DlHandlePtr,
        err: UserSliceMut,
        unwind: &'a wasixv2_exec::GuestUnwind,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(self.open(
            env,
            caller,
            file,
            flags,
            search,
            caller_addr,
            handle,
            err,
            Some(unwind),
        ))
    }
    fn dl_valid_handle<'a>(
        &'a self,
        env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
        caller: &'a Caller<'_>,
        handle: DlHandle,
    ) -> LocalFuture<'a, SysResult<bool>> {
        Box::pin(async move {
            self.ensure_live().map_err(|_| Errno::ESTALE)?;
            let gate = self.root.gate().map_err(|_| Errno::ENOSYS)?;
            if !env
                .read()
                .await
                .data()
                .syscalls()
                .loader_gate(caller.task())
                .is_some_and(|current| Arc::ptr_eq(&current, gate))
            {
                return Err(Errno::ESTALE.into());
            }
            let _admission = match gate.admit() {
                Ok(value) => value,
                Err(_) => {
                    self.service(&Context::Import(env))
                        .await
                        .map_err(|_| Errno::EINTR)?;
                    return Err(wasixv2_syscalls::SysErr::RestartNoIntr);
                }
            };
            Ok(handle.0 > 1 && (handle.0 as usize) <= self.root.graph().libraries.len())
        })
    }
    fn dl_sym<'a>(
        &'a self,
        env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
        caller: &'a Caller<'_>,
        handle: DlHandle,
        symbol: Cstr,
        caller_addr: Uptr,
        addr: UptrPtr,
        err: UserSliceMut,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async move {
            self.ensure_live().map_err(|_| Errno::ESTALE)?;
            if caller_addr != 0 {
                return load::report(
                    caller,
                    err,
                    "caller-sensitive lookup unsupported",
                    Errno::EOPNOTSUPP,
                )
                .await;
            }
            let gate = self.root.gate().map_err(|_| Errno::ENOSYS)?;
            if !env
                .read()
                .await
                .data()
                .syscalls()
                .loader_gate(caller.task())
                .is_some_and(|current| Arc::ptr_eq(&current, gate))
            {
                return Err(Errno::ESTALE.into());
            }
            let _admission = match gate.admit() {
                Ok(value) => value,
                Err(_) => {
                    self.service(&Context::Import(env))
                        .await
                        .map_err(|_| Errno::EINTR)?;
                    return Err(wasixv2_syscalls::SysErr::RestartNoIntr);
                }
            };
            let name = read_cstr(caller.memory(), symbol, 4096)
                .await
                .map_err(|_| Errno::EFAULT)?;
            let name = std::str::from_utf8(&name).map_err(|_| Errno::EINVAL)?;
            let graph = self.root.graph();
            let native_index = handle.0.checked_sub(1).map(|index| index as usize);
            if !self.root.policy.resident_handles() && native_index.is_none() {
                return Err(Errno::EINVAL.into());
            }
            let native_scope = native_index
                .filter(|index| *index < graph.libraries.len())
                .map(|index| {
                    let mut scope = if index == 0 {
                        graph.global.clone()
                    } else {
                        Vec::new()
                    };
                    for dependency in graph.scope(index) {
                        if !scope.contains(&dependency) {
                            scope.push(dependency);
                        }
                    }
                    scope
                })
                .unwrap_or_default();
            let Some(scope) = self
                .root
                .policy
                .scope(handle.0, graph.libraries.len(), native_scope)
            else {
                return load::report(caller, err, "invalid library handle", Errno::EINVAL).await;
            };
            if scope.iter().any(|index| *index >= graph.libraries.len()) {
                return Err(Errno::EINVAL.into());
            }
            if !self.root.policy.resident_handles()
                && native_index.is_none_or(|index| {
                    lock(&self.root.state)
                        .references
                        .get(&index)
                        .copied()
                        .unwrap_or(0)
                        == 0
                })
            {
                return load::report(caller, err, "invalid library handle", Errno::EINVAL).await;
            }
            let address = Context::Import(env)
                .with(|_, store| lock(&self.local).symbol_in_scope(store, &graph, &scope, name))
                .await;
            match address {
                Ok(value) => {
                    write_value(caller.memory(), addr, &value)
                        .await
                        .map_err(|_| Errno::EFAULT)?;
                    Ok(0)
                }
                Err(error) => load::report(caller, err, &error.to_string(), Errno::ENOENT).await,
            }
        })
    }
    fn dl_close<'a>(
        &'a self,
        env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
        caller: &'a Caller<'_>,
        handle: DlHandle,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async move {
            self.ensure_live().map_err(|_| Errno::ESTALE)?;
            let gate = self.root.gate().map_err(|_| Errno::ENOSYS)?;
            if !env
                .read()
                .await
                .data()
                .syscalls()
                .loader_gate(caller.task())
                .is_some_and(|current| Arc::ptr_eq(&current, gate))
            {
                return Err(Errno::ESTALE.into());
            }
            let _admission = match gate.admit() {
                Ok(value) => value,
                Err(_) => {
                    self.service(&Context::Import(env))
                        .await
                        .map_err(|_| Errno::EINTR)?;
                    return Err(wasixv2_syscalls::SysErr::RestartNoIntr);
                }
            };
            let Some(index) = handle.0.checked_sub(1) else {
                return Ok(-Errno::EINVAL.0);
            };
            let mut state = lock(&self.root.state);
            match state.references.get_mut(&(index as usize)) {
                Some(count) if *count > 0 => {
                    *count -= 1;
                    Ok(0)
                }
                _ => Ok(-Errno::EINVAL.0),
            }
        })
    }
    fn dl_next_init<'a>(
        &'a self,
        env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
        caller: &'a Caller<'_>,
        ctors: UptrPtr,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async move {
            self.ensure_live().map_err(|_| Errno::ESTALE)?;
            let gate = self.root.gate().map_err(|_| Errno::ENOSYS)?;
            if !env
                .read()
                .await
                .data()
                .syscalls()
                .loader_gate(caller.task())
                .is_some_and(|current| Arc::ptr_eq(&current, gate))
            {
                return Err(Errno::ESTALE.into());
            }
            let _admission = match gate.admit() {
                Ok(value) => value,
                Err(_) => {
                    self.service(&Context::Import(env))
                        .await
                        .map_err(|_| Errno::EINTR)?;
                    return Err(wasixv2_syscalls::SysErr::RestartNoIntr);
                }
            };
            let next = lock(&self.root.state).ctors.pop_front();
            if let Some(value) = next {
                if write_value(caller.memory(), ctors, &value).await.is_err() {
                    lock(&self.root.state).ctors.push_front(value);
                    return Err(Errno::EFAULT.into());
                }
                Ok(1)
            } else {
                Ok(0)
            }
        })
    }
    fn start<'a>(&'a self, entry: &'a Entry<'a, S>) -> LocalFuture<'a, Result<()>> {
        Box::pin(async move {
            self.ensure_live()?;
            self.inner.start(entry).await
        })
    }
    fn on_entry_return(&self) -> EntryReturn {
        self.inner.on_entry_return()
    }
    fn start_thread<'a>(
        &'a self,
        entry: &'a Entry<'a, S>,
        tid: i32,
        arg: u32,
    ) -> LocalFuture<'a, Result<()>> {
        Box::pin(async move {
            self.ensure_live()?;
            self.inner.start_thread(entry, tid, arg).await
        })
    }
    fn on_thread_return(&self) -> EntryReturn {
        self.inner.on_thread_return()
    }
    fn deliver_signal<'a>(
        &'a self,
        env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
        frame: &'a SignalFrame,
        restart: bool,
    ) -> LocalFuture<'a, Result<Sigreturn>> {
        Box::pin(async move {
            self.ensure_live()?;
            self.inner.deliver_signal(env, frame, restart).await
        })
    }
    fn vfork_resume<'a>(
        &'a self,
        env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
        child: TaskId,
        pid: i32,
    ) -> LocalFuture<'a, VforkResume> {
        Box::pin(async move {
            if let Err(error) = self.ensure_live() {
                return VforkResume::Unwind(error);
            }
            self.inner.vfork_resume(env, child, pid).await
        })
    }
    fn memory_limit(&self) -> u64 {
        self.inner.memory_limit()
    }
    fn as_any(&self) -> &dyn Any {
        self.inner.as_any()
    }
}
impl<S: 'static> Drop for StoreImage<S> {
    fn drop(&mut self) {
        let mut local = lock(&self.local);
        let mut leases = std::mem::take(&mut local.leases);
        if let Some(stage) = local.stage.take() {
            leases.extend(stage.leases);
        }
        if leases.is_empty() {
            return;
        }
        if let Some(cleanup) = self.cleanup.get() {
            let owner = self.root.owner.clone();
            cleanup.spawn(Box::pin(async move {
                for lease in leases {
                    lease.release().await;
                }
                drop(owner);
            }));
        }
    }
}

#[cfg(test)]
mod admission_tests {
    use super::*;
    #[test]
    fn imports_finish_original_admission_after_a_loader_closes_entry() {
        let root = Root::new(crate::tests::bytes("(module)", [0; 4], &[], &[], &[]));
        let gate = LoaderGate::new(71);
        let admitted = gate.claim_image(&root.owner).unwrap();
        admitted.bind(TaskId(3)).unwrap();
        assert!(root.gate.set(gate.clone()).is_ok());
        let closing = gate.close(TaskId(2)).unwrap();
        assert!(!gate.ready_for(LoaderWait::Drained));
        root.admitted_store(&gate).unwrap();
        assert!(root.admitted_store(&LoaderGate::new(72)).is_err());
        drop(admitted);
        assert!(gate.ready_for(LoaderWait::Drained));
        drop(closing);
    }
}

#[cfg(test)]
mod lease_cancellation_tests {
    use super::*;
    use std::{
        cell::RefCell,
        future::Future,
        task::{Context, Poll, Waker},
        time::Duration,
    };
    use wasixv2_host::{Executor, Host};
    thread_local! {
        static JOBS: RefCell<Vec<LocalFuture<'static, ()>>> = RefCell::new(Vec::new());
    }
    // The existing exec NoThreads test host shape, with a manually driven local queue.
    struct QueueHost;
    impl Host for QueueHost {
        fn realtime(&self) -> Duration {
            Duration::ZERO
        }
        fn monotonic(&self) -> Duration {
            Duration::ZERO
        }
        fn sleep(&self, _: Duration) -> LocalFuture<'_, ()> {
            Box::pin(async {})
        }
        fn random(&self, _: &mut [u8]) {}
        fn executor(&self) -> &dyn Executor {
            self
        }
    }
    impl Executor for QueueHost {
        fn spawn_local(&self, future: LocalFuture<'static, ()>) {
            JOBS.with(|jobs| jobs.borrow_mut().push(future));
        }
        fn spawn_blocking(&self, work: Box<dyn FnOnce() + Send>) -> LocalFuture<'static, ()> {
            Box::pin(async move { work() })
        }
    }
    struct Lease {
        ready: Arc<AtomicBool>,
        completed: Arc<AtomicBool>,
    }
    impl LoaderReservation for Lease {
        fn address(&self) -> u32 {
            0
        }
        fn length(&self) -> u32 {
            1
        }
        fn retain(&mut self) {}
        fn release(self: Box<Self>) -> LocalFuture<'static, ()> {
            Box::pin(async move {
                std::future::poll_fn(|_| {
                    if self.ready.load(Ordering::Acquire) {
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                })
                .await;
                self.completed.store(true, Ordering::Release);
            })
        }
    }
    #[test]
    fn canceled_startup_releases_mapping_before_reopening_and_dropping_nonce() {
        let owner = Arc::new(());
        let weak = Arc::downgrade(&owner);
        let gate = LoaderGate::new(91);
        let task = TaskId(1);
        let admission = gate.claim_image(&owner).unwrap();
        admission.bind(task).unwrap();
        let closing = gate.close(task).unwrap();
        drop(admission);
        let scope = wasixv2_host::LoaderCleanup::new(Arc::new(QueueHost));
        let ready = Arc::new(AtomicBool::new(false));
        let completed = Arc::new(AtomicBool::new(false));
        drop(startup::Pending {
            cleanup: scope.clone(),
            owner,
            closing: Some(closing),
            leases: vec![Box::new(Lease {
                ready: ready.clone(),
                completed: completed.clone(),
            })],
        });
        let mut cx = Context::from_waker(Waker::noop());
        let mut drain = Box::pin(scope.run(async {}));
        assert!(drain.as_mut().poll(&mut cx).is_pending());
        let mut job = JOBS.with(|jobs| jobs.borrow_mut().pop().unwrap());
        assert!(job.as_mut().poll(&mut cx).is_pending());
        assert!(gate.busy());
        assert!(weak.upgrade().is_some());
        assert!(!completed.load(Ordering::Acquire));
        ready.store(true, Ordering::Release);
        assert!(job.as_mut().poll(&mut cx).is_ready());
        drop(job);
        assert!(completed.load(Ordering::Acquire));
        assert!(!gate.busy());
        assert!(weak.upgrade().is_none());
        assert!(drain.as_mut().poll(&mut cx).is_ready());
    }
    #[test]
    fn weak_logical_retirement_releases_before_retained_image_handle_drops() {
        use wasixv2_syscalls::LoaderControl;
        struct Members(std::sync::atomic::AtomicUsize);
        impl LoaderControl for Members {
            fn loader_members(&self, _: &LoaderGate) -> Option<usize> {
                Some(self.0.load(Ordering::Acquire))
            }
        }
        let host: Arc<dyn Host> = Arc::new(QueueHost);
        let root = Root::new(crate::tests::bytes("(module)", [0; 4], &[], &[], &[]));
        assert!(root.gate.set(LoaderGate::new(81)).is_ok());
        #[cfg(feature = "engine-tests")]
        let code = {
            let store = Store::default();
            let mut graph = (*root.graph()).clone();
            graph.libraries.push(
                Library::new(crate::tests::bytes("(module)", [0; 4], &[], &[], &[]), None).unwrap(),
            );
            root.prepare_code(&store, &mut graph).unwrap();
            let weak = Arc::downgrade(graph.libraries[1].prepared.as_ref().unwrap());
            lock(&root.state).graph = Arc::new(graph);
            weak
        };
        let nonce = Arc::downgrade(&root.owner);
        let completed = Arc::new(AtomicBool::new(false));
        let ready = Arc::new(AtomicBool::new(false));
        lock(&root.state).leases.push(Box::new(Lease {
            ready: ready.clone(),
            completed: completed.clone(),
        }));
        let members = Arc::new(Members(std::sync::atomic::AtomicUsize::new(1)));
        let first = wasixv2_host::LoaderCleanup::new(host.clone());
        root.register_retirement(&first, members.clone()).unwrap();
        let mut cx = Context::from_waker(Waker::noop());
        let mut prior = Box::pin(first.run(async {}));
        assert!(prior.as_mut().poll(&mut cx).is_pending());
        let mut callback = JOBS.with(|jobs| jobs.borrow_mut().pop().unwrap());
        assert!(callback.as_mut().poll(&mut cx).is_ready());
        drop(callback);
        assert!(prior.as_mut().poll(&mut cx).is_ready());
        drop(prior);
        assert!(!root.retired.load(Ordering::Acquire));
        assert_eq!(lock(&root.state).leases.len(), 1);
        #[cfg(feature = "engine-tests")]
        assert!(
            code.upgrade().is_some(),
            "surviving members retain compiled code"
        );
        // An unstarted/surviving old-mm user keeps the image live across first-runner exec.
        members.0.store(0, Ordering::Release);
        let final_scope = wasixv2_host::LoaderCleanup::new(host);
        root.register_retirement(&final_scope, members).unwrap();
        let mut final_run = Box::pin(final_scope.run(async {}));
        assert!(final_run.as_mut().poll(&mut cx).is_pending());
        let mut callback = JOBS.with(|jobs| jobs.borrow_mut().pop().unwrap());
        assert!(callback.as_mut().poll(&mut cx).is_pending());
        assert!(root.retired.load(Ordering::Acquire));
        #[cfg(feature = "engine-tests")]
        {
            assert!(
                code.upgrade().is_none(),
                "retirement releases both graph and cache ownership"
            );
            assert!(lock(&root.artifacts).is_empty());
            assert_eq!(root.graph().libraries.len(), 2, "metadata can outlive code");
        }
        assert!(!completed.load(Ordering::Acquire));
        assert!(nonce.upgrade().is_some());
        ready.store(true, Ordering::Release);
        assert!(callback.as_mut().poll(&mut cx).is_ready());
        drop(callback);
        assert!(final_run.as_mut().poll(&mut cx).is_ready());
        assert!(completed.load(Ordering::Acquire));
        assert!(nonce.upgrade().is_some());
        drop(root);
        assert!(nonce.upgrade().is_none());
    }
    #[test]
    fn final_common_release_uses_surviving_runner_after_old_scope_retires() {
        let host: Arc<dyn Host> = Arc::new(QueueHost);
        let first = wasixv2_host::LoaderCleanup::new(host.clone());
        let surviving = wasixv2_host::LoaderCleanup::new(host);
        let root = Root::new(crate::tests::bytes("(module)", [0; 4], &[], &[], &[]));
        assert!(root.host.set(Arc::new(QueueHost)).is_ok());
        root.attach_cleanup(&first);
        root.attach_cleanup(&surviving);
        let nonce = Arc::downgrade(&root.owner);
        let completed = Arc::new(AtomicBool::new(false));
        let ready = Arc::new(AtomicBool::new(false));
        lock(&root.state).leases.push(Box::new(Lease {
            ready: ready.clone(),
            completed: completed.clone(),
        }));
        let mut cx = Context::from_waker(Waker::noop());
        let mut retired = Box::pin(first.run(async {}));
        assert!(retired.as_mut().poll(&mut cx).is_ready());
        drop(retired);
        assert!(!first.is_open());
        // Root surviving an exec/CLONE_VM detachment must not keep the old runner alive.
        assert!(nonce.upgrade().is_some());
        drop(root);
        let mut run = Box::pin(surviving.run(async {}));
        assert!(run.as_mut().poll(&mut cx).is_pending());
        let mut job = JOBS.with(|jobs| jobs.borrow_mut().pop().unwrap());
        assert!(job.as_mut().poll(&mut cx).is_pending());
        assert!(nonce.upgrade().is_some());
        ready.store(true, Ordering::Release);
        assert!(job.as_mut().poll(&mut cx).is_ready());
        drop(job);
        assert!(run.as_mut().poll(&mut cx).is_ready());
        assert!(completed.load(Ordering::Acquire));
        assert!(nonce.upgrade().is_none());
    }
    #[test]
    fn partial_prepare_leases_remain_in_stage_until_awaited_rollback() {
        let root = Root::new(crate::tests::bytes("(module)", [0; 4], &[], &[], &[]));
        let completed = Arc::new(AtomicBool::new(false));
        let ready = Arc::new(AtomicBool::new(false));
        let mut stage = None;
        LocalStage::resume(&mut stage, vec![0], Vec::new());
        stage.as_mut().unwrap().leases.push(Box::new(Lease {
            ready: ready.clone(),
            completed: completed.clone(),
        }));
        // A later allocation or instantiation fails. Refreshing the TLS snapshot
        // must preserve already allocated leases for this same rollback stage.
        LocalStage::resume(&mut stage, vec![0, 4096], Vec::new());
        assert_eq!(stage.as_ref().unwrap().leases.len(), 1);
        assert!(!completed.load(Ordering::Acquire));
        assert!(JOBS.with(|jobs| jobs.borrow().is_empty()));
        let ledger = LeaseLedger {
            cleanup: wasixv2_host::LoaderCleanup::new(Arc::new(QueueHost)),
            root,
            leases: stage.take().unwrap().leases,
            pending: None,
        };
        let mut rollback = Box::pin(ledger.release());
        let mut cx = Context::from_waker(Waker::noop());
        assert!(rollback.as_mut().poll(&mut cx).is_pending());
        assert!(!completed.load(Ordering::Acquire));
        ready.store(true, Ordering::Release);
        assert!(rollback.as_mut().poll(&mut cx).is_ready());
        assert!(completed.load(Ordering::Acquire));
    }
    #[test]
    fn canceled_peer_rollback_transfers_consumed_release_and_retains_nonce() {
        let root = Root::new(crate::tests::bytes("(module)", [0; 4], &[], &[], &[]));
        assert!(root.host.set(Arc::new(QueueHost)).is_ok());
        let nonce = Arc::downgrade(&root.owner);
        let ready = Arc::new(AtomicBool::new(false));
        let completed = Arc::new(AtomicBool::new(false));
        let ledger = LeaseLedger {
            cleanup: wasixv2_host::LoaderCleanup::new(Arc::new(QueueHost)),
            root: root.clone(),
            pending: None,
            leases: vec![Box::new(Lease {
                ready: ready.clone(),
                completed: completed.clone(),
            })],
        };
        let mut release = Box::pin(ledger.release());
        let mut cx = Context::from_waker(Waker::noop());
        assert!(release.as_mut().poll(&mut cx).is_pending());
        drop(release);
        drop(root);
        assert!(nonce.upgrade().is_some());
        let mut transferred = JOBS
            .with(|jobs| jobs.borrow_mut().pop())
            .expect("cleanup transferred");
        assert!(transferred.as_mut().poll(&mut cx).is_pending());
        ready.store(true, Ordering::Release);
        assert!(transferred.as_mut().poll(&mut cx).is_ready());
        drop(transferred);
        assert!(completed.load(Ordering::Acquire));
        assert!(nonce.upgrade().is_none());
    }
}

#[cfg(all(
    test,
    any(feature = "engine-tests", all(feature = "js", target_arch = "wasm32"))
))]
mod lazy_tests {
    use super::*;
    use std::{
        future::Future,
        task::{Context as PollContext, Waker},
    };
    use wasmer::{AsStoreAsync, TypedFunction};

    fn library(wat: &str) -> Library {
        let wat = wat.replacen(
            "(module",
            r#"(module (import "env" "memory" (memory 1 4 shared))"#,
            1,
        );
        Library::new(crate::tests::bytes(&wat, [0; 4], &[], &[], &[]), None).unwrap()
    }
    fn prepare(
        local: &mut Local,
        store: &mut Store,
        imports: &Imports,
        graph: Graph,
        previous: usize,
    ) -> Work {
        let mut work = cyclic_tests::work(graph, store);
        work.previous = previous;
        let count = work.graph().unwrap().libraries.len();
        local
            .stage_modules(
                &mut store.as_store_mut(),
                imports,
                &work,
                vec![0; count],
                Vec::new(),
                TaskId(1),
            )
            .unwrap();
        work
    }
    fn publish(local: &mut Local, store: &mut Store) {
        local
            .stage
            .as_mut()
            .unwrap()
            .apply(store, &local.table)
            .unwrap();
        let stage = local.stage.take().unwrap();
        for module in stage.modules.into_values() {
            local.instances.push(module.publish().unwrap());
        }
        local.forwarders.extend(stage.forwarders);
        local.tls = stage.tls;
        local.lazy_targets = stage.lazy_targets;
    }
    fn wasm_replacement(store: &mut Store, increment: i32) -> BoundModule {
        // A JS funcref table accepts actual Wasm functions, not arbitrary host JS
        // closures. Keep its compiled module alive through the same bound owner as
        // production code, while preserving exact observed-slot rollback assertions.
        let module = Module::new(
            &*store,
            wat::parse_str(format!(
                r#"(module
            (func (export "replacement") (param i32) (result i32)
                local.get 0 i32.const {increment} i32.add))"#
            ))
            .unwrap(),
        )
        .unwrap();
        let instance = Instance::new(store, &module, &Imports::new()).unwrap();
        BoundModule::new(&instance)
    }
    fn assert_unresolved_trap(error: RuntimeError) {
        #[cfg(not(all(feature = "js", target_arch = "wasm32")))]
        assert_eq!(
            error.to_trap(),
            Some(wasmer_types::TrapCode::UnreachableCodeReached)
        );
        #[cfg(all(feature = "js", target_arch = "wasm32"))]
        {
            use js_sys::wasm_bindgen::{JsCast, JsValue};
            let thrown: JsValue = error.into();
            assert!(
                thrown.is_instance_of::<js_sys::WebAssembly::RuntimeError>(),
                "the unresolved Wasm body must produce a real engine RuntimeError"
            );
        }
    }
    fn initial() -> (Graph, Local, Store, Imports, TypedFunction<i32, i32>, u32) {
        let (mut graph, mut local, mut store, imports) = cyclic_tests::fixture();
        graph.libraries.truncate(1);
        graph.cursor = 3;
        graph.initialization = vec![0];
        graph.libraries.push(library(
            r#"(module
            (import "env" "late" (func $late (param i32) (result i32)))
            (func (export "call") (param i32) (result i32) local.get 0 call $late))"#,
        ));
        graph.assign_slots(1).unwrap();
        graph.seal_lazy(1, 1).unwrap();
        let slot = graph.libraries[1].lazy.values().next().unwrap().slot;
        prepare(&mut local, &mut store, &imports, graph.clone(), 1);
        publish(&mut local, &mut store);
        let call = local.instances[1]
            .exports
            .get_typed_function::<i32, i32>(&store, "call")
            .unwrap();
        let Some(Value::FuncRef(Some(stub))) = local.table.get(&mut store, slot) else {
            panic!("unresolved slot must contain a real typed trap, never null");
        };
        // Prove the installed target itself traps with the expected signature;
        // this excludes a null/ill-typed indirect call as a substitute for the stub.
        assert_unresolved_trap(
            stub.typed::<i32, i32>(&store)
                .unwrap()
                .call(&mut store, 1)
                .unwrap_err(),
        );
        assert_unresolved_trap(call.call(&mut store, 1).unwrap_err());
        (graph, local, store, imports, call, slot)
    }
    fn resolved(mut graph: Graph) -> Graph {
        graph.libraries.push(library("(module (func (export \"late\") (param i32) (result i32) local.get 0 i32.const 10 i32.add))"));
        graph.assign_slots(2).unwrap();
        graph.global.push(2);
        graph.seal(2, 2).unwrap();
        graph
    }
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn deferred_slots_restore_exact_old_values_after_partial_apply_and_replay_future_stores() {
        let (graph, mut local, mut store, imports, call, slot) = initial();
        let old_image = wasm_replacement(&mut store, 3);
        let old = old_image
            .exports
            .get_function("replacement")
            .unwrap()
            .clone();
        local
            .table
            .set(&mut store, slot, Value::FuncRef(Some(old.clone())))
            .unwrap();
        let graph = resolved(graph);
        prepare(&mut local, &mut store, &imports, graph.clone(), 2);
        local.stage.as_mut().unwrap().lazy_slots.push(LazySlot {
            slot: u32::MAX,
            before: Value::FuncRef(None),
            function: old,
            applied: false,
        });
        assert!(
            local
                .stage
                .as_mut()
                .unwrap()
                .apply(&mut store, &local.table)
                .is_err()
        );
        assert_eq!(call.call(&mut store, 2).unwrap(), 12);
        local
            .stage
            .as_mut()
            .unwrap()
            .rollback(&mut store, &local.table)
            .unwrap();
        // Exact observed old funcref, not a freshly recreated unresolved stub or null.
        assert_eq!(call.call(&mut store, 2).unwrap(), 5);
        local.stage.take();
        assert_eq!(local.lazy_targets[&slot], None);
        prepare(&mut local, &mut store, &imports, graph.clone(), 2);
        publish(&mut local, &mut store);
        assert_eq!(call.call(&mut store, 2).unwrap(), 12);
        assert!(local.lazy_targets[&slot].is_some());
        let (_, mut peer, mut peer_store, peer_imports) = cyclic_tests::fixture();
        prepare(&mut peer, &mut peer_store, &peer_imports, graph, 1);
        publish(&mut peer, &mut peer_store);
        assert_eq!(
            peer.instances[1]
                .exports
                .get_typed_function::<i32, i32>(&peer_store, "call")
                .unwrap()
                .call(&mut peer_store, 4)
                .unwrap(),
            14
        );
    }
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn a_failed_peer_restores_each_existing_stores_observed_slot() {
        let (graph, mut first, mut first_store, imports, first_call, slot) = initial();
        let (_, mut second, mut second_store, second_imports, second_call, _) = initial();
        let graph = resolved(graph);
        let old_first_image = wasm_replacement(&mut first_store, 3);
        let old_second_image = wasm_replacement(&mut second_store, 7);
        let old_first = old_first_image
            .exports
            .get_function("replacement")
            .unwrap()
            .clone();
        let old_second = old_second_image
            .exports
            .get_function("replacement")
            .unwrap()
            .clone();
        first
            .table
            .set(&mut first_store, slot, Value::FuncRef(Some(old_first)))
            .unwrap();
        second
            .table
            .set(
                &mut second_store,
                slot,
                Value::FuncRef(Some(old_second.clone())),
            )
            .unwrap();
        prepare(&mut first, &mut first_store, &imports, graph.clone(), 2);
        prepare(&mut second, &mut second_store, &second_imports, graph, 2);
        first
            .stage
            .as_mut()
            .unwrap()
            .apply(&mut first_store, &first.table)
            .unwrap();
        second.stage.as_mut().unwrap().lazy_slots.push(LazySlot {
            slot: u32::MAX,
            before: Value::FuncRef(None),
            function: old_second,
            applied: false,
        });
        assert!(
            second
                .stage
                .as_mut()
                .unwrap()
                .apply(&mut second_store, &second.table)
                .is_err()
        );
        for (local, store) in [
            (&mut first, &mut first_store),
            (&mut second, &mut second_store),
        ] {
            local
                .stage
                .as_mut()
                .unwrap()
                .rollback(store, &local.table)
                .unwrap();
            assert_eq!(local.lazy_targets[&slot], None);
        }
        assert_eq!(first_call.call(&mut first_store, 2).unwrap(), 5);
        assert_eq!(second_call.call(&mut second_store, 2).unwrap(), 9);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn local_promotion_resolves_lazy_while_old_now_stays_frozen_on_existing_and_future_stores() {
        let (mut graph, mut local, mut store, imports, call, slot) = initial();
        graph.libraries.push(library(
            r#"(module
            (import "env" "choice" (func $choice (result i32)))
            (func (export "choice") (result i32) i32.const 7)
            (func (export "now_call") (result i32) call $choice))"#,
        ));
        graph.assign_slots(2).unwrap();
        graph.seal(2, 2).unwrap();
        let frozen = graph.libraries[2].fixups.clone();
        prepare(&mut local, &mut store, &imports, graph.clone(), 2);
        publish(&mut local, &mut store);
        let now = local.instances[2]
            .exports
            .get_typed_function::<(), i32>(&store, "now_call")
            .unwrap();
        assert_eq!(now.call(&mut store).unwrap(), 7);

        graph.libraries.push(library(
            r#"(module
            (func (export "late") (param i32) (result i32) local.get 0 i32.const 10 i32.add)
            (func (export "choice") (result i32) i32.const 99))"#,
        ));
        graph.assign_slots(3).unwrap();
        graph.seal(3, 3).unwrap();
        assert_eq!(
            graph.libraries[1].lazy.values().next().unwrap().target,
            None
        );
        prepare(&mut local, &mut store, &imports, graph.clone(), 3);
        publish(&mut local, &mut store);
        assert_unresolved_trap(call.call(&mut store, 1).unwrap_err());
        assert_eq!(now.call(&mut store).unwrap(), 7);
        assert_eq!(
            local.instances[3]
                .exports
                .get_typed_function::<(), i32>(&store, "choice")
                .unwrap()
                .call(&mut store)
                .unwrap(),
            99
        );

        graph.global.push(3);
        graph.seal(3, 4).unwrap();
        assert_eq!(graph.libraries[2].fixups, frozen);
        prepare(&mut local, &mut store, &imports, graph.clone(), 4);
        assert!(
            local.stage.as_ref().unwrap().modules.is_empty(),
            "promotion must not create a duplicate instance"
        );
        assert_eq!(local.stage.as_ref().unwrap().lazy_slots.len(), 1);
        publish(&mut local, &mut store);
        assert_eq!(call.call(&mut store, 4).unwrap(), 14);
        assert_eq!(now.call(&mut store).unwrap(), 7);
        assert_eq!(local.lazy_targets[&slot], Some((3, 0)));

        let (_, mut peer, mut peer_store, peer_imports) = cyclic_tests::fixture();
        prepare(&mut peer, &mut peer_store, &peer_imports, graph, 1);
        publish(&mut peer, &mut peer_store);
        assert_eq!(
            peer.instances[1]
                .exports
                .get_typed_function::<i32, i32>(&peer_store, "call")
                .unwrap()
                .call(&mut peer_store, 6)
                .unwrap(),
            16
        );
        assert_eq!(
            peer.instances[2]
                .exports
                .get_typed_function::<(), i32>(&peer_store, "now_call")
                .unwrap()
                .call(&mut peer_store)
                .unwrap(),
            7
        );
        assert_eq!(peer.lazy_targets[&slot], Some((3, 0)));
    }

    struct PendingLease(Arc<AtomicBool>);
    impl LoaderReservation for PendingLease {
        fn address(&self) -> u32 {
            0
        }
        fn length(&self) -> u32 {
            1
        }
        fn retain(&mut self) {}
        fn release(self: Box<Self>) -> LocalFuture<'static, ()> {
            Box::pin(std::future::poll_fn(move |_| {
                if self.0.load(Ordering::Acquire) {
                    std::task::Poll::Ready(())
                } else {
                    std::task::Poll::Pending
                }
            }))
        }
    }
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn canceled_cleanup_restores_old_slots_before_awaiting_the_original_release() {
        struct Control;
        impl wasixv2_syscalls::LoaderControl for Control {}
        let (graph, mut local, mut store, imports, call, slot) = initial();
        let old_image = wasm_replacement(&mut store, 3);
        let old = old_image
            .exports
            .get_function("replacement")
            .unwrap()
            .clone();
        local
            .table
            .set(&mut store, slot, Value::FuncRef(Some(old)))
            .unwrap();
        let work = prepare(&mut local, &mut store, &imports, resolved(graph), 2);
        local
            .stage
            .as_mut()
            .unwrap()
            .apply(&mut store, &local.table)
            .unwrap();
        let ready = Arc::new(AtomicBool::new(false));
        local
            .stage
            .as_mut()
            .unwrap()
            .leases
            .push(Box::new(PendingLease(ready.clone())));
        let root = Root::new(crate::tests::bytes("(module)", [0; 4], &[], &[], &[]));
        let gate = LoaderGate::new(107);
        assert!(root.gate.set(gate.clone()).is_ok());
        let task = TaskId(1);
        drop(gate.bind(task).unwrap());
        let transaction = gate.close(task).unwrap().activate(&[task]).unwrap();
        gate.acknowledge(task, gate.command(task).unwrap()).unwrap();
        for phase in [LoaderPhase::Prepare, LoaderPhase::Apply] {
            transaction.advance(phase).unwrap();
            gate.acknowledge(task, gate.command(task).unwrap()).unwrap();
        }
        lock(&root.state).work = Some(Arc::new(work));
        drop(transaction);
        let store = store.into_async();
        let local = Arc::new(Mutex::new(local));
        let mut pending = Some(Box::pin(load::cleanup(
            root,
            local.clone(),
            store.store(),
            Arc::new(Control),
            task,
        )));
        let mut cx = PollContext::from_waker(Waker::noop());
        let mut abort = Box::pin(async { pending.as_mut().unwrap().await });
        assert!(abort.as_mut().poll(&mut cx).is_pending());
        drop(abort);
        assert!(gate.busy());
        assert_eq!(call.call(&mut store.store().write(), 2).unwrap(), 5);
        assert_eq!(lock(&local).lazy_targets[&slot], None);
        let mut transferred = pending.take().unwrap();
        ready.store(true, Ordering::Release);
        assert!(matches!(
            transferred.as_mut().poll(&mut cx),
            std::task::Poll::Ready(Ok(()))
        ));
        assert!(!gate.busy());
    }
}

#[cfg(all(
    test,
    any(feature = "engine-tests", all(feature = "js", target_arch = "wasm32"))
))]
mod main_tls_tests {
    use super::*;

    fn fixture(offset: u32) -> (Graph, Local, Store, Imports) {
        let (mut graph, mut local, mut store, imports) = cyclic_tests::fixture();
        let main = crate::tests::bytes(
            &format!(
                r#"(module
                (global (export "__stack_pointer") (mut i32) (i32.const 65536))
                (global (export "__tls_base") (mut i32) (i32.const 0))
                (global (export "__tls_size") i32 (i32.const 16))
                (global (export "__tls_align") i32 (i32.const 4))
                (global (export "main_value") i32 (i32.const {offset})))"#
            ),
            [0; 4],
            &[],
            &[("main_value", wasmparser::SymbolFlags::TLS)],
            &[],
        );
        let module = Module::new(&store, &main).unwrap();
        let instance = Instance::new(&mut store, &module, &Imports::new()).unwrap();
        local.instances[0] = BoundModule::new(&instance);
        graph.libraries = vec![Library::new(main, None).unwrap()];
        graph.cursor = 3;
        graph.initialization = vec![0];
        graph.libraries.push(
            Library::new(
                crate::tests::bytes(
                    r#"(module
                (import "env" "memory" (memory 1 4 shared))
                (import "GOT.mem" "main_value" (global $value (mut i32)))
                (func (export "address") (result i32) global.get $value)
                (func (export "field") (result i32) global.get $value i32.const 4 i32.add)
                (func (export "read") (result i32) global.get $value i32.load)
                (func (export "write") (param i32) global.get $value local.get 0 i32.store))"#,
                    [0; 4],
                    &[],
                    &[],
                    &[],
                ),
                None,
            )
            .unwrap(),
        );
        graph.assign_slots(1).unwrap();
        graph.seal(1, 1).unwrap();
        (graph, local, store, imports)
    }

    fn publish(local: &mut Local, store: &mut Store, imports: &Imports, work: &Work) {
        local
            .stage_modules(
                &mut store.as_store_mut(),
                imports,
                work,
                vec![0; 2],
                Vec::new(),
                TaskId(1),
            )
            .unwrap();
        local
            .stage
            .as_mut()
            .unwrap()
            .apply(store, &local.table)
            .unwrap();
        let stage = local.stage.take().unwrap();
        for module in stage.modules.into_values() {
            local.instances.push(module.publish().unwrap());
        }
        local.forwarders.extend(stage.forwarders);
        local.tls = stage.tls;
        local.lazy_targets = stage.lazy_targets;
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn code_reads_follow_real_main_global_before_and_after_each_stores_tls_bootstrap() {
        let (graph, mut local, mut store, imports) = fixture(8);
        let work = cyclic_tests::work(graph, &store);
        publish(&mut local, &mut store, &imports, &work);
        let (_, mut peer, mut peer_store, peer_imports) = fixture(8);
        publish(&mut peer, &mut peer_store, &peer_imports, &work);
        for (local, store, first, second) in [
            (&mut local, &mut store, 4096u32, 8192u32),
            (&mut peer, &mut peer_store, 12288u32, 16384u32),
        ] {
            let main = local.instances[0]
                .exports
                .get_global("__tls_base")
                .unwrap()
                .clone();
            let code = &local.instances[1].exports;
            let address = code
                .get_typed_function::<(), i32>(&*store, "address")
                .unwrap();
            let field = code
                .get_typed_function::<(), i32>(&*store, "field")
                .unwrap();
            let read = code.get_typed_function::<(), i32>(&*store, "read").unwrap();
            let write = code
                .get_typed_function::<i32, ()>(&*store, "write")
                .unwrap();
            // Instantiation/Apply happened before bootstrap, with a zero main base.
            // The same code follows later CLONE_SETTLS/inherited-base assignments.
            for (base, value) in [(first, 41i32), (second, 73i32)] {
                main.set(&mut *store, Value::I32(base as i32)).unwrap();
                local
                    .memory
                    .view(&*store)
                    .write(u64::from(base + 8), &value.to_le_bytes())
                    .unwrap();
                assert_eq!(address.call(&mut *store).unwrap(), (base + 8) as i32);
                assert_eq!(field.call(&mut *store).unwrap(), (base + 12) as i32);
                assert_eq!(read.call(&mut *store).unwrap(), value);
                write.call(&mut *store, value + 1).unwrap();
                assert_eq!(read.call(&mut *store).unwrap(), value + 1);
                assert_eq!(
                    local
                        .symbol(
                            &mut store.as_store_mut(),
                            &work.graph().unwrap(),
                            0,
                            "main_value"
                        )
                        .unwrap(),
                    base + 8
                );
            }
            let mut old = [0; 4];
            local
                .memory
                .view(&*store)
                .read(u64::from(first + 8), &mut old)
                .unwrap();
            assert_eq!(
                i32::from_le_bytes(old),
                42,
                "later base changes never rewrite captured pointer memory"
            );
        }
        assert_eq!(
            local.instances[0]
                .exports
                .get_global("__tls_base")
                .unwrap()
                .get(&mut store),
            Value::I32(8192)
        );
        assert_eq!(
            peer.instances[0]
                .exports
                .get_global("__tls_base")
                .unwrap()
                .get(&mut peer_store),
            Value::I32(16384)
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn cache_distinguishes_checked_offset_variants_without_keying_on_store_bases() {
        let (mut graph, _, store, _) = fixture(8);
        let root = Root::new(graph.libraries[0].source.to_vec());
        root.prepare_code(&store, &mut graph).unwrap();
        let original = graph.libraries[1].prepared.clone().unwrap();
        let (_, _, peer, _) = fixture(8);
        root.prepare_code(&peer, &mut graph).unwrap();
        assert!(Arc::ptr_eq(
            &original,
            graph.libraries[1].prepared.as_ref().unwrap()
        ));
        let (mut other, _, _, _) = fixture(12);
        root.prepare_code(&store, &mut other).unwrap();
        assert!(!Arc::ptr_eq(
            &original,
            other.libraries[1].prepared.as_ref().unwrap()
        ));
        assert_eq!(lock(&root.artifacts).len(), 2);
        let (invalid, _, _, _) = fixture(16);
        assert!(invalid.main_tls_imports(1).is_err());
        let target = &invalid.libraries[1]
            .fixups
            .iter()
            .find(|fixup| fixup.kind == FixupKind::GotMemory)
            .unwrap()
            .target;
        assert!(invalid.target_address(target, &[4096, 0]).is_err());
    }
}

#[cfg(all(test, feature = "engine-tests"))]
mod symbol_tests {
    use super::*;
    use wasmer::{AsStoreMut, MemoryType, Store, TableType};

    #[test]
    fn main_tls_lookup_reads_live_global_in_each_store_without_changing_bindings() {
        for base in [2048, 4096] {
            let source = crate::tests::bytes(
                &format!(
                    r#"(module
                    (global (export "__tls_base") (mut i32) (i32.const {base}))
                    (global (export "value") i32 (i32.const 8)))"#
                ),
                [0; 4],
                &[],
                &[("value", wasmparser::SymbolFlags::TLS)],
                &[],
            );
            let graph = Graph {
                libraries: vec![Library::new(source.clone(), None).unwrap()],
                global: vec![0],
                cursor: 3,
                initialization: vec![0],
            };
            let mut store = Store::default();
            let module = Module::new(&store, source).unwrap();
            let instance = Instance::new(&mut store, &module, &Imports::new()).unwrap();
            let global = instance.exports.get_global("__tls_base").unwrap().clone();
            let memory = Memory::new(&mut store, MemoryType::new(1, Some(4), true)).unwrap();
            let table = Table::new(
                &mut store,
                TableType::new(Type::FuncRef, 3, Some(64)),
                Value::FuncRef(None),
            )
            .unwrap();
            let local = Local {
                forward_code: Arc::default(),
                instances: vec![BoundModule::new(&instance)],
                forwarders: Vec::new(),
                tls: vec![123456],
                memory,
                table,
                stage: None,
                leases: Vec::new(),
                generation: 0,
                lazy_targets: BTreeMap::new(),
            };
            for value in [base, 8192] {
                global.set(&mut store, Value::I32(value)).unwrap();
                assert_eq!(
                    local
                        .symbol(&mut store.as_store_mut(), &graph, 0, "value")
                        .unwrap(),
                    value as u32 + 8
                );
            }
            global.set(&mut store, Value::I32(0)).unwrap();
            assert!(
                local
                    .symbol(&mut store.as_store_mut(), &graph, 0, "value")
                    .is_err()
            );
            assert_eq!(local.tls, [123456]);
        }
    }
}

#[cfg(all(
    test,
    any(feature = "engine-tests", all(feature = "js", target_arch = "wasm32"))
))]
mod cyclic_tests {
    use super::*;
    use wasmer::{AsStoreMut, Function, MemoryType, Store, TableType};

    pub(super) fn fixture() -> (Graph, Local, Store, Imports) {
        let mut store = Store::default();
        let memory = Memory::new(&mut store, MemoryType::new(1, Some(4), true)).unwrap();
        let table = Table::new(
            &mut store,
            TableType::new(Type::FuncRef, 3, Some(64)),
            Value::FuncRef(None),
        )
        .unwrap();
        let main = Module::new(
            &store,
            wat::parse_str(
                "(module (global (export \"__stack_pointer\") (mut i32) (i32.const 65536)))",
            )
            .unwrap(),
        )
        .unwrap();
        let main = Instance::new(&mut store, &main, &Imports::new()).unwrap();
        let library = |name: &str, other: &str, increment: u32| {
            let extra = if name == "a" {
                r#"(import "test" "require" (func))"#
            } else {
                ""
            };
            Library::new(
                crate::tests::bytes(
                    &format!(
                        r#"(module
                (import "env" "memory" (memory 1 4 shared))
                (import "env" "__indirect_function_table" (table 3 funcref))
                (import "env" "__table_base" (global $base i32))
                (import "env" "{other}" (func $other (param i32 i64) (result i64)))
                {extra}
                (export "{other}_alias" (func $other))
                (elem (global.get $base) func $other)
                (func (export "{name}") (param $n i32) (param $x i64) (result i64)
                    local.get $n i32.eqz
                    if (result i64) local.get $x
                    else local.get $n i32.const 1 i32.sub
                         local.get $x i64.const {increment} i64.add call $other end))"#
                    ),
                    [0, 0, 1, 0],
                    &[other],
                    &[],
                    &[],
                ),
                None,
            )
            .unwrap()
        };
        let mut graph = Graph {
            libraries: vec![
                Library::new(crate::tests::bytes("(module)", [0; 4], &[], &[], &[]), None).unwrap(),
                library("a", "b", 1),
                library("b", "a", 2),
            ],
            global: vec![0],
            cursor: 3,
            initialization: vec![0],
        };
        graph.libraries[1].dependencies = vec![2];
        graph.libraries[2].dependencies = vec![1];
        for index in 1..3 {
            graph.libraries[index].table = graph.cursor;
            graph.cursor += 1;
            graph.assign_slots(index).unwrap();
        }
        graph.seal(1, 1).unwrap();
        let mut imports = Imports::new();
        imports.define("env", "memory", memory.clone());
        imports.define("env", "__indirect_function_table", table.clone());
        imports.define("test", "require", Function::new_typed(&mut store, || {}));
        // Same environment-backed hook types as staging's engine sibling. No hook
        // may run at instantiation/Apply, and guest calls are only made after publish.
        for source in &graph.libraries[1..] {
            let instrumented =
                wasixv2_exec::instrument(&source.initialization.instantiate_bytes().unwrap())
                    .unwrap();
            let module = Module::new(&store, instrumented).unwrap();
            for import in module
                .imports()
                .functions()
                .filter(|i| i.module() == "wasixv2-exec")
            {
                let function =
                    Function::new(&mut store, import.ty(), |_| Ok(vec![Value::I32(65536)]));
                imports.define(import.module(), import.name(), function);
            }
        }
        let local = Local {
            forward_code: Arc::default(),
            instances: vec![BoundModule::new(&main)],
            forwarders: Vec::new(),
            memory,
            table,
            tls: vec![0],
            stage: None,
            leases: Vec::new(),
            generation: 0,
            lazy_targets: BTreeMap::new(),
        };
        (graph, local, store, imports)
    }
    pub(super) fn work(mut graph: Graph, store: &Store) -> Work {
        Root::new(graph.libraries[0].source.to_vec())
            .prepare_code(store, &mut graph)
            .unwrap();
        Work {
            publication: Mutex::new(Some(Publication {
                graph: Arc::new(graph),
                references: BTreeMap::new(),
                ctors: VecDeque::new(),
            })),
            previous: 1,
            first: TaskId(1),
            leases: Mutex::new(Vec::new()),
            committed: AtomicBool::new(false),
            error: Mutex::new(None),
        }
    }
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn image_code_cache_reuses_artifacts_and_distinguishes_replaced_sources() {
        let (mut graph, _, store, _) = fixture();
        let root = Root::new(graph.libraries[0].source.to_vec());
        root.prepare_code(&store, &mut graph).unwrap();
        let old = graph.libraries[1].prepared.clone().unwrap();
        let mut replay = graph.clone();
        replay.libraries[1].prepared = None;
        root.prepare_code(&Store::new(store.engine().clone()), &mut replay)
            .unwrap();
        assert!(Arc::ptr_eq(
            &old,
            replay.libraries[1].prepared.as_ref().unwrap()
        ));
        assert_eq!(lock(&root.artifacts).len(), 2);
        replay.libraries[1] = Library::new(
            crate::tests::bytes("(module (func))", [0; 4], &[], &[], &[]),
            None,
        )
        .unwrap();
        root.prepare_code(&store, &mut replay).unwrap();
        assert!(!Arc::ptr_eq(
            &old,
            replay.libraries[1].prepared.as_ref().unwrap()
        ));
        assert_eq!(lock(&root.artifacts).len(), 3);
        lock(&root.artifacts).clear();
        assert_eq!(
            old.module().exports().functions().count(),
            graph.libraries[1]
                .prepared
                .as_ref()
                .unwrap()
                .module()
                .exports()
                .functions()
                .count()
        );
    }
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn now_cycles_publish_concrete_aliases_and_replay_in_future_stores() {
        // Two independent stores replay identical frozen targets; a shared Rust
        // Function/Instance is never transferred between them.
        for task in [TaskId(1), TaskId(2)] {
            let (graph, mut local, mut store, imports) = fixture();
            let work = work(graph, &store);
            local
                .stage_modules(
                    &mut store.as_store_mut(),
                    &imports,
                    &work,
                    vec![0; 3],
                    Vec::new(),
                    task,
                )
                .unwrap();
            let stage = local.stage.as_mut().unwrap();
            assert_eq!(stage.forwarders.len(), 1);
            assert!(wasixv2_exec::is_preemptible(stage.forwarders[0].module()));
            assert!(matches!(
                local.table.get(&mut store, 5),
                Some(Value::FuncRef(None))
            ));
            for module in stage.modules.values_mut() {
                module.apply(&mut store).unwrap();
            }
            let stage = local.stage.take().unwrap();
            local.forwarders.extend(stage.forwarders);
            for module in stage.modules.into_values() {
                local.instances.push(module.publish().unwrap());
            }
            assert_eq!(
                local.instances[1]
                    .exports
                    .get_typed_function::<(i32, i64), i64>(&store, "a")
                    .unwrap()
                    .call(&mut store, 6, 10)
                    .unwrap(),
                19
            );
            assert_eq!(
                local.instances[2]
                    .exports
                    .get_typed_function::<(i32, i64), i64>(&store, "b")
                    .unwrap()
                    .call(&mut store, 5, 10)
                    .unwrap(),
                18
            );
            let graph = work.graph().unwrap();
            let a = graph.symbol(1, "a", &[0; 3]).unwrap();
            assert_eq!(graph.symbol(2, "a_alias", &[0; 3]).unwrap(), a);
            let alias_slot = graph.libraries[2].slots[&0];
            let Some(Value::FuncRef(Some(function))) = local.table.get(&mut store, alias_slot)
            else {
                panic!("published imported alias missing")
            };
            assert_eq!(
                function
                    .typed::<(i32, i64), i64>(&store)
                    .unwrap()
                    .call(&mut store, 4, 1)
                    .unwrap(),
                7
            );
            // Table::get can materialize a different Rust handle for the same
            // funcref. Distinguish the definition from its forwarding alias by
            // changing the canonical target after publication: the definition
            // keeps returning x, whereas the thunk would now return -999.
            let replacement = Module::new(&store, wat::parse_str("(module (func (export \"replace\") (param i32 i64) (result i64) i64.const -999))").unwrap()).unwrap();
            let replacement = Instance::new(&mut store, &replacement, &Imports::new()).unwrap();
            let replacement = replacement.exports.get_function("replace").unwrap().clone();
            local
                .table
                .set(&mut store, a, Value::FuncRef(Some(replacement)))
                .unwrap();
            assert_eq!(
                function
                    .typed::<(i32, i64), i64>(&store)
                    .unwrap()
                    .call(&mut store, 0, 123)
                    .unwrap(),
                123
            );
        }
    }
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn forwarding_preserves_numeric_multivalue_and_native_exceptions() {
        let (_, local, mut store, imports) = fixture();
        let provider = Module::new(
            &store,
            wat::parse_str(
                r#"(module
            (tag $error (export "error") (param i32))
            (func (export "many") (param i32 i64 f32 f64) (result i32 i64 f32 f64)
                local.get 0 local.get 1 local.get 2 local.get 3)
            (func (export "raise") (param i32) (result i32) local.get 0 throw $error))"#,
            )
            .unwrap(),
        )
        .unwrap();
        let provider = Instance::new(&mut store, &provider, &Imports::new()).unwrap();
        let targets = [
            (
                wasmparser::FuncType::new(
                    [
                        wasmparser::ValType::I32,
                        wasmparser::ValType::I64,
                        wasmparser::ValType::F32,
                        wasmparser::ValType::F64,
                    ],
                    [
                        wasmparser::ValType::I32,
                        wasmparser::ValType::I64,
                        wasmparser::ValType::F32,
                        wasmparser::ValType::F64,
                    ],
                ),
                3,
            ),
            (
                wasmparser::FuncType::new([wasmparser::ValType::I32], [wasmparser::ValType::I32]),
                4,
            ),
        ];
        local
            .table
            .grow(&mut store, 2, Value::FuncRef(None))
            .unwrap();
        let forwarder = forward::instantiate(
            &local.forward_code,
            &mut store,
            &local.table,
            &imports,
            &targets,
        )
        .unwrap();
        // Preparation creates code but leaves every target unpublished.
        assert!(matches!(
            local.table.get(&mut store, 3),
            Some(Value::FuncRef(None))
        ));

        for (slot, name) in [(3, "many"), (4, "raise")] {
            local
                .table
                .set(
                    &mut store,
                    slot,
                    Value::FuncRef(Some(provider.exports.get_function(name).unwrap().clone())),
                )
                .unwrap();
        }
        let input = [
            Value::I32(-17),
            Value::I64(1 << 40),
            Value::F32(3.25),
            Value::F64(-7.5),
        ];
        assert_eq!(
            forwarder
                .exports
                .get_function(&forward::name(0))
                .unwrap()
                .call(&mut store, &input)
                .unwrap()
                .as_ref(),
            &input
        );
        let catcher = Module::new(
            &store,
            wat::parse_str(
                r#"(module
            (import "env" "raise" (func $raise (param i32) (result i32)))
            (import "env" "error" (tag $error (param i32)))
            (func (export "catch") (param i32) (result i32)
                (block $caught (result i32)
                    (try_table (result i32) (catch $error $caught)
                        local.get 0 call $raise))))"#,
            )
            .unwrap(),
        )
        .unwrap();
        let mut imports = Imports::new();
        imports.define(
            "env",
            "raise",
            forwarder
                .exports
                .get_function(&forward::name(1))
                .unwrap()
                .clone(),
        );
        imports.define(
            "env",
            "error",
            provider.exports.get::<Extern>("error").unwrap().clone(),
        );
        let catcher = Instance::new(&mut store, &catcher, &imports).unwrap();
        assert_eq!(
            catcher
                .exports
                .get_typed_function::<i32, i32>(&store, "catch")
                .unwrap()
                .call(&mut store, 73)
                .unwrap(),
            73
        );
        assert!(
            forward::instantiate(
                &local.forward_code,
                &mut store,
                &local.table,
                &Imports::new(),
                &[(targets[1].0.clone(), 2)]
            )
            .is_err()
        );
    }
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn cyclic_apply_rollback_clears_all_new_targets_and_drops_forwarders() {
        let (graph, mut local, mut store, imports) = fixture();
        let work = work(graph, &store);
        local
            .stage_modules(
                &mut store.as_store_mut(),
                &imports,
                &work,
                vec![0; 3],
                Vec::new(),
                TaskId(1),
            )
            .unwrap();
        let stage = local.stage.as_mut().unwrap();
        assert_eq!(stage.forwarders.len(), 1);
        for module in stage.modules.values_mut() {
            module.apply(&mut store).unwrap();
        }
        for module in stage.modules.values_mut() {
            module.rollback(&mut store).unwrap();
        }
        drop(local.stage.take());
        for slot in 3..work.graph().unwrap().cursor {
            assert!(matches!(
                local.table.get(&mut store, slot),
                Some(Value::FuncRef(None))
            ));
        }
        assert!(local.forwarders.is_empty());
        assert_eq!(local.instances.len(), 1);
        // The identical load is retryable after rollback; no orphan slot is occupied.
        local
            .stage_modules(
                &mut store.as_store_mut(),
                &imports,
                &work,
                vec![0; 3],
                Vec::new(),
                TaskId(1),
            )
            .unwrap();
    }
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn failed_cyclic_prepare_preserves_staged_ownership_and_old_table() {
        let (graph, mut local, mut store, mut imports) = fixture();
        // A fails after B and its forwarding instance were successfully prepared.
        // JS host functions are untyped, so use a wrong import kind on both backends.
        imports.define("test", "require", Global::new(&mut store, Value::I32(0)));
        let work = work(graph, &store);
        assert!(
            local
                .stage_modules(
                    &mut store.as_store_mut(),
                    &imports,
                    &work,
                    vec![0; 3],
                    Vec::new(),
                    TaskId(1)
                )
                .is_err()
        );
        assert_eq!(local.instances.len(), 1);
        assert!(local.forwarders.is_empty());
        assert_eq!(local.stage.as_ref().unwrap().forwarders.len(), 1);
        assert!(local.stage.as_ref().unwrap().modules.contains_key(&2));
        for slot in 3..local.table.size(&store) {
            assert!(matches!(
                local.table.get(&mut store, slot),
                Some(Value::FuncRef(None))
            ));
        }
    }
}
