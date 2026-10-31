//! Native per-image loader. Shared state contains plans/source, never store-local functions.
mod graph;
mod load;
use crate::{BindingTarget, ExternType, FixupKind, Initialization, SOURCE_SECTION, StagedModule};
use graph::{Graph, Library, infrastructure, initial_globals};
use std::{
    any::Any,
    collections::{BTreeMap, VecDeque},
    ops::Range,
    sync::{
        Arc, Mutex, MutexGuard, OnceLock,
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
    AsStoreMut, AsyncFunctionEnvMut, Extern, FunctionEnv, Global, Imports, Instance, Memory,
    Module, Mutability, RuntimeError, Store, StoreMut, Table, Type, Value,
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

/// Register before Wasixv2. Only explicitly source-prepared native images opt in.
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
        let source = module.custom_sections(SOURCE_SECTION).next()?.to_vec();
        let root = Root::new(source);
        if let Err(error) = verify_compiled_source(module) {
            let _ = root.error.set(error.to_string());
        }
        Some(Arc::new(Image {
            root,
            _type: std::marker::PhantomData,
        }))
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
    if module.sys_artifact().is_none() {
        return Err(failure("native dynamic image requires sys backend"));
    }
    let mut sections = module.custom_sections(SOURCE_SECTION);
    let source = sections
        .next()
        .ok_or_else(|| failure("missing validated source"))?
        .to_vec();
    if sections.next().is_some() {
        return Err(failure("duplicate source marker"));
    }
    let expected = crate::prepare_image(&source)
        .map_err(failure)
        .and_then(|bytes| wasixv2_exec::instrument(&bytes).map_err(failure))?;
    if module.info().hash != Some(wasmer_types::ModuleHash::new(expected))
        || module.info().start_function.is_some()
        || !module.info().table_initializers.is_empty()
    {
        return Err(failure(
            "compiled image differs from validated no-auto-start source",
        ));
    }
    Ok(source)
}
struct Root {
    owner: Arc<()>,
    gate: OnceLock<Arc<LoaderGate>>,
    host: OnceLock<Arc<dyn wasixv2_host::Host>>,
    error: OnceLock<String>,
    initialized: AtomicBool,
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
    fn new(source: Vec<u8>) -> Arc<Self> {
        let parsed = (|| -> Result<(Graph, u32)> {
            let mut main = Library::new(source, None)?;
            if !main.metadata.needed().is_empty() {
                return Err(failure("startup dependencies are not supported"));
            }
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
            owner: Arc::new(()),
            gate: OnceLock::new(),
            host: OnceLock::new(),
            error,
            initialized: AtomicBool::new(false),
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
        if let Some(host) = self.host.get() {
            let owner = self.owner.clone();
            host.executor().spawn_local(Box::pin(async move {
                for lease in leases {
                    lease.release().await;
                }
                drop(owner);
            }));
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
    _type: std::marker::PhantomData<S>,
}
impl<S: Syscalls + Control + Send + Sync + 'static> Abi<S> for Image<S> {
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
        if self.root.error.get().is_some() {
            return Err(Errno::ENOEXEC);
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
        let mut imports = <Wasixv2 as Abi<S>>::imports(&Wasixv2, store, env, module);
        let data = env.as_ref(store);
        let task = data.task();
        let syscalls = data.syscalls().clone();
        let host = data.host().clone();
        let setup = (|| -> Result<()> {
            if let Some(error) = self.root.error.get() {
                return Err(failure(error));
            }
            if !syscalls.loader_fatal_supported() {
                return Err(failure("image-wide fatal failure control is unavailable"));
            }
            let gate = syscalls
                .loader_gate(task)
                .ok_or_else(|| failure("dynamic image requires mm control"))?;
            self.root.admitted_store(&gate)?;
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
    fn bind(
        &self,
        instance: &Instance,
        store: &mut Store,
        task: TaskId,
        role: Role,
    ) -> std::result::Result<Arc<dyn AbiInstance<S>>, ExecError> {
        if let Some(error) = self.root.error.get() {
            return Err(ExecError::Load(wasixv2_exec::LoadError::other(failure(
                error,
            ))));
        }
        let inner = <Wasixv2 as Abi<S>>::bind(&Wasixv2, instance, store, task, role)?;
        let memory = instance
            .exports
            .get_memory("__wasix_dylink_memory_0")?
            .clone();
        let table = instance
            .exports
            .get_table("__indirect_function_table")?
            .clone();
        Ok(Arc::new(StoreImage {
            root: self.root.clone(),
            inner,
            role,
            local: Arc::new(Mutex::new(Local {
                instances: vec![instance.clone()],
                tls: vec![0],
                memory,
                table,
                stage: None,
                leases: Vec::new(),
                generation: 0,
            })),
            _type: std::marker::PhantomData,
        }))
    }
    fn image_layout(
        &self,
        _: &Instance,
        memory: &Memory,
        store: &mut Store,
        _: bool,
    ) -> Option<wasixv2_syscalls::MemoryLayout> {
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
        true
    }
    fn low_guard(&self) -> bool {
        true
    }
}
struct Local {
    instances: Vec<Instance>,
    tls: Vec<u32>,
    memory: Memory,
    table: Table,
    stage: Option<LocalStage>,
    leases: Vec<Box<dyn LoaderReservation>>,
    generation: u64,
}
struct LeaseLedger {
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
        if let Some(pending) = self.take_pending()
            && let Some(host) = self.root.host.get()
        {
            let owner = self.root.owner.clone();
            host.executor().spawn_local(Box::pin(async move {
                pending.await;
                drop(owner);
            }));
        }
    }
}
struct LocalStage {
    modules: BTreeMap<usize, StagedModule>,
    tls: Vec<u32>,
    leases: Vec<Box<dyn LoaderReservation>>,
}
struct StoreImage<S: 'static> {
    root: Arc<Root>,
    inner: Arc<dyn AbiInstance<S>>,
    role: Role,
    local: Arc<Mutex<Local>>,
    _type: std::marker::PhantomData<S>,
}
impl<S: Syscalls + Control + Send + Sync + 'static> StoreImage<S> {
    fn main_prepare(&self, store: &mut StoreMut<'_>) -> Result<()> {
        let graph = self.root.graph();
        let main = &graph.libraries[0];
        let mut local = lock(&self.local);
        let instance = &local.instances[0];
        let inputs = engine_globals(instance, store)?;
        let init = Initialization::parse(&main.source).map_err(failure)?;
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
        env: &AsyncFunctionEnvMut<ExecEnv<S>>,
        caller: &Caller<'_>,
        work: &Work,
    ) -> Result<()> {
        let graph = work.graph()?;
        let (syscalls, current_tls) = {
            let data = env.read().await;
            (
                data.data().syscalls().clone(),
                lock(&self.local).tls.clone(),
            )
        };
        let mut tls = current_tls;
        tls.resize(graph.libraries.len(), 0);
        let mut ledger = LeaseLedger {
            root: self.root.clone(),
            leases: Vec::new(),
            pending: None,
        };
        for (index, tls_base) in tls.iter_mut().enumerate().skip(work.previous) {
            let lib = &graph.libraries[index];
            if caller.task() == work.first {
                if let Some(symbol) = lib.metadata.exports().get("__tls_base") {
                    let globals =
                        initial_globals(&lib.source, infrastructure(lib, self.root.stack, 0))?;
                    let end =
                        u64::from(lib.base) + u64::from(lib.metadata.memory_info().memory_size);
                    let effects = Initialization::parse(&lib.source)
                        .map_err(failure)?
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
                ledger.leases.push(lease);
            }
        }
        let mut guard = env.write().await;
        let (data, store) = guard.data_and_store_mut();
        self.stage_modules(
            &mut store.as_store_mut(),
            data.imports(),
            work,
            tls,
            std::mem::take(&mut ledger.leases),
            caller.task(),
        )
    }
    fn stage_modules(
        &self,
        store: &mut StoreMut<'_>,
        base_imports: &Imports,
        work: &Work,
        tls: Vec<u32>,
        leases: Vec<Box<dyn LoaderReservation>>,
        task: TaskId,
    ) -> Result<()> {
        lock(&self.local).stage = Some(LocalStage {
            modules: BTreeMap::new(),
            tls: tls.clone(),
            leases,
        });
        let mut local = lock(&self.local);
        let graph = work.graph()?;
        local
            .instances
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
            for (ordinal, import) in lib.metadata.imports().iter().enumerate() {
                let fixup = lib
                    .fixups
                    .iter()
                    .find(|f| f.module == index && f.import == ordinal);
                let value = match &import.ty {
                    ExternType::Memory(_) => Some(Extern::Memory(local.memory.clone())),
                    ExternType::Table(_) => Some(Extern::Table(local.table.clone())),
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
                    ExternType::Function(_) if fixup.is_some_and(|f| f.kind != FixupKind::Host) => {
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
                                .map(StagedModule::instance)
                        }
                        .ok_or_else(|| failure("cyclic direct function dependency"))?;
                        Some(Extern::Function(engine_function(provider, *index)?))
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
            let fresh_memory = if task == work.first {
                u64::from(lib.base)
                    ..u64::from(lib.base) + u64::from(lib.metadata.memory_info().memory_size)
            } else {
                u64::from(tls[index])..u64::from(tls[index]) + u64::from(lib.tls_size)
            };
            let fresh_table = u64::from(lib.table)..u64::from(graph.cursor);
            let mut prepared = StagedModule::prepare(
                store,
                &lib.source,
                &imports,
                local.memory.clone(),
                local.table.clone(),
                std::slice::from_ref(&fresh_memory),
                std::slice::from_ref(&fresh_table),
                if task == work.first {
                    None
                } else {
                    Some(tls[index])
                },
            )?;
            for (function, slot) in &lib.slots {
                prepared.bind_slot(store, *slot, *function, std::slice::from_ref(&fresh_table))?;
            }
            local
                .stage
                .as_mut()
                .unwrap()
                .modules
                .insert(index, prepared);
        }
        Ok(())
    }
    async fn execute_command(
        &self,
        env: &AsyncFunctionEnvMut<ExecEnv<S>>,
        task: TaskId,
        command: wasixv2_syscalls::LoaderCommand,
    ) -> Result<()> {
        let work = lock(&self.root.state).work.clone();
        match command.phase {
            LoaderPhase::Park => {}
            LoaderPhase::Prepare => {
                let memory = wasixv2_exec::GuestMemory::new(env);
                let caller = Caller::new(task, &memory);
                self.prepare_modules(
                    env,
                    &caller,
                    work.as_deref()
                        .ok_or_else(|| failure("missing load work"))?,
                )
                .await?;
            }
            LoaderPhase::Apply => {
                let mut guard = env.write().await;
                let (_, store) = guard.data_and_store_mut();
                let mut local = lock(&self.local);
                for staged in local
                    .stage
                    .as_mut()
                    .ok_or_else(|| failure("missing prepared store"))?
                    .modules
                    .values_mut()
                {
                    staged.apply(store)?;
                }
            }
            LoaderPhase::Publish => {
                let mut local = lock(&self.local);
                if let Some(mut staged) = local.stage.take() {
                    local.leases.extend(std::mem::take(&mut staged.leases));
                    for module in staged.modules.into_values() {
                        local.instances.push(module.publish()?);
                    }
                    local.tls = staged.tls;
                    local.leases.extend(staged.leases);
                }
                local.generation = self.root.gate()?.generation() + 1;
            }
            LoaderPhase::Rollback => {
                let stage = {
                    let mut guard = env.write().await;
                    let (_, store) = guard.data_and_store_mut();
                    let mut local = lock(&self.local);
                    if let Some(staged) = &mut local.stage {
                        for module in staged.modules.values_mut() {
                            module.rollback(store)?;
                        }
                    }
                    local.stage.take()
                };
                if let Some(stage) = stage {
                    LeaseLedger {
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
        env: &AsyncFunctionEnvMut<ExecEnv<S>>,
        task: TaskId,
        command: wasixv2_syscalls::LoaderCommand,
    ) -> Result<()> {
        if let Err(error) = self.execute_command(env, task, command).await {
            if matches!(command.phase, LoaderPhase::Prepare | LoaderPhase::Apply) {
                if let Some(work) = lock(&self.root.state).work.as_ref() {
                    let mut saved = lock(&work.error);
                    if saved.is_none() {
                        *saved = Some(error.to_string());
                    }
                }
            } else {
                let syscalls = env.read().await.data().syscalls().clone();
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
fn engine_globals(instance: &Instance, store: &mut impl AsStoreMut) -> Result<BTreeMap<u32, u64>> {
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
    instance: &Instance,
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
fn engine_function(instance: &Instance, index: u32) -> Result<wasmer::Function> {
    instance
        .exports
        .get_function(&format!("__wasix_dylink_fn_{index}"))
        .cloned()
        .map_err(failure)
}
impl<S: Syscalls + Control + Send + Sync + 'static> AbiInstance<S> for StoreImage<S> {
    fn prepare_entry<'a>(&'a self, entry: &'a Entry<'a, S>) -> LocalFuture<'a, Result<()>> {
        Box::pin(async move {
            entry
                .with_store(|_, store| self.main_prepare(store))
                .await?;
            let graph = self.root.graph();
            if graph.libraries.len() > 1 {
                let (syscalls, task) = entry
                    .with_store(|data, _| (data.syscalls().clone(), data.task()))
                    .await;
                let memory = entry.memory();
                let caller = Caller::new(task, &memory);
                let mut tls = lock(&self.local).tls.clone();
                tls.resize(graph.libraries.len(), 0);
                let mut ledger = LeaseLedger {
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
                            data.imports(),
                            &work,
                            tls,
                            std::mem::take(&mut ledger.leases),
                            task,
                        )
                    })
                    .await?;
                entry
                    .with_store(|_, store| {
                        let mut local = lock(&self.local);
                        let stage = local
                            .stage
                            .as_mut()
                            .ok_or_else(|| failure("missing entry stage"))?;
                        for module in stage.modules.values_mut() {
                            module.apply(store)?;
                        }
                        let mut stage = local.stage.take().unwrap();
                        local.leases.extend(std::mem::take(&mut stage.leases));
                        for module in stage.modules.into_values() {
                            local.instances.push(module.publish()?);
                        }
                        local.tls = stage.tls;
                        local.leases.extend(stage.leases);
                        local.generation = self.root.gate()?.generation();
                        Ok::<(), RuntimeError>(())
                    })
                    .await?;
            }
            Ok(())
        })
    }
    fn loader_work<'a>(
        &'a self,
        env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
    ) -> LocalFuture<'a, Result<()>> {
        Box::pin(self.service(env))
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
        Box::pin(self.open(env, caller, file, flags, search, caller_addr, handle, err))
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
                    self.service(env).await.map_err(|_| Errno::EINTR)?;
                    return Err(wasixv2_syscalls::SysErr::RestartNoIntr);
                }
            };
            let name = read_cstr(caller.memory(), symbol, 4096)
                .await
                .map_err(|_| Errno::EFAULT)?;
            let name = std::str::from_utf8(&name).map_err(|_| Errno::EINVAL)?;
            let index = handle.0.checked_sub(1).ok_or(Errno::EINVAL)? as usize;
            if lock(&self.root.state)
                .references
                .get(&index)
                .copied()
                .unwrap_or(0)
                == 0
            {
                return load::report(caller, err, "invalid library handle", Errno::EINVAL).await;
            }
            let graph = self.root.graph();
            if index >= graph.libraries.len() {
                return Err(Errno::EINVAL.into());
            }
            let address = {
                let local = lock(&self.local);
                graph.symbol(index, name, &local.tls)
            };
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
                    self.service(env).await.map_err(|_| Errno::EINTR)?;
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
                    self.service(env).await.map_err(|_| Errno::EINTR)?;
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
        self.inner.start(entry)
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
        self.inner.start_thread(entry, tid, arg)
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
        self.inner.deliver_signal(env, frame, restart)
    }
    fn vfork_resume<'a>(
        &'a self,
        env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
        child: TaskId,
        pid: i32,
    ) -> LocalFuture<'a, VforkResume> {
        self.inner.vfork_resume(env, child, pid)
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
        if let Some(host) = self.root.host.get() {
            let owner = self.root.owner.clone();
            host.executor().spawn_local(Box::pin(async move {
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
    fn canceled_peer_rollback_transfers_consumed_release_and_retains_nonce() {
        let root = Root::new(crate::tests::bytes("(module)", [0; 4], &[], &[], &[]));
        assert!(root.host.set(Arc::new(QueueHost)).is_ok());
        let nonce = Arc::downgrade(&root.owner);
        let ready = Arc::new(AtomicBool::new(false));
        let completed = Arc::new(AtomicBool::new(false));
        let ledger = LeaseLedger {
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
