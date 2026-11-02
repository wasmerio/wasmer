//! EH/PIC startup uses the existing graph, allocator and transaction before guest entry.
use super::*;
use wasixv2_exec::{Preparation, PreparedImage};
use wasixv2_syscalls::{LoaderClosing, LoaderStartup, MemoryLayout};

pub(super) type PreparedResult<S> =
    std::result::Result<Option<Box<dyn PreparedImage<S>>>, ExecError>;

fn load_error(error: impl std::fmt::Debug) -> ExecError {
    ExecError::Load(wasixv2_exec::LoadError::other(failure(error)))
}

struct StartupPolicy {
    loader: Arc<dyn LoadPolicy>,
    search: Option<Vec<u8>>,
    preloads: Vec<Vec<u8>>,
}
impl StartupPolicy {
    #[cfg(test)]
    fn new(startup: &LoaderStartup) -> Self {
        Self::for_loader(startup, Arc::new(policy::NativePolicy))
    }
    fn for_loader(startup: &LoaderStartup, loader: Arc<dyn LoadPolicy>) -> Self {
        let value = |name: &[u8]| {
            startup
                .environment
                .iter()
                .find_map(|entry| entry.strip_prefix(name))
        };
        let mut preloads = Vec::new();
        // musl ldso/dynlink.c:__dls3 trusts neither environment knob in secure mode.
        if !startup.secure
            && let Some(preload) = value(b"LD_PRELOAD=").filter(|_| loader.preloads())
        {
            preloads.extend(
                preload
                    .split(|b| b.is_ascii_whitespace() || *b == b':')
                    .filter(|name| !name.is_empty())
                    .map(<[u8]>::to_vec),
            );
        }
        let search = loader.startup_search(if startup.secure {
            None
        } else {
            value(b"LD_LIBRARY_PATH=")
        });
        Self {
            loader,
            search,
            preloads,
        }
    }
}

/// The exact closing token stays in cleanup custody until every unpublished mapping
/// is released. The owner nonce, not a strong Root pin, survives a canceled prepare.
pub(super) struct Pending {
    pub(super) cleanup: Arc<wasixv2_host::LoaderCleanup>,
    pub(super) owner: Arc<()>,
    pub(super) closing: Option<LoaderClosing>,
    pub(super) leases: Vec<Box<dyn LoaderReservation>>,
}
impl Drop for Pending {
    fn drop(&mut self) {
        let leases = std::mem::take(&mut self.leases);
        let closing = self.closing.take();
        if leases.is_empty() {
            drop(closing);
            return;
        }
        let owner = self.owner.clone();
        self.cleanup.spawn(Box::pin(async move {
            for lease in leases {
                lease.release().await;
            }
            drop(closing);
            drop(owner);
        }));
    }
}

struct Prepared<S: 'static> {
    root: Arc<Root>,
    base: Arc<dyn Abi<S>>,
    imports: Imports,
    forwarders: Vec<BoundModule>,
    tls: Vec<u32>,
    bound: Option<Bound<S>>,
}
pub(super) struct Bound<S: 'static> {
    graph: Arc<Graph>,
    pending: Pending,
    env: FunctionEnv<ExecEnv<S>>,
    process: bool,
}
impl<S: Syscalls + Control + Send + Sync + 'static> PreparedImage<S> for Prepared<S> {
    fn imports(&self) -> &Imports {
        &self.imports
    }
    fn bind(
        mut self: Box<Self>,
        instance: &Instance,
        store: &mut Store,
        task: TaskId,
        role: Role,
    ) -> std::result::Result<Arc<dyn AbiInstance<S>>, ExecError> {
        let image = Image::<S> {
            root: self.root.clone(),
            base: self.base.clone(),
        };
        let bound = image.bind_store(instance, store, task, role)?;
        {
            let mut local = lock(&bound.local);
            local.forwarders.append(&mut self.forwarders);
            local.tls = std::mem::take(&mut self.tls);
            // A peer owns only its private TLS; initial shared data stays transaction-owned.
            if let Some(startup) = &mut self.bound
                && !startup.process
            {
                local.leases.append(&mut startup.pending.leases);
            }
        }
        *lock(&bound.startup) = self.bound.take();
        Ok(bound)
    }
}

impl<S: Syscalls + Control + Send + Sync + 'static> Image<S> {
    pub(super) async fn prepare_startup(&self, cx: &Preparation<'_, S>) -> PreparedResult<S> {
        if let Some(error) = self.root.error.get() {
            return Err(load_error(error));
        }
        let (syscalls, task, cleanup, base) = cx
            .with_store(|data, _| {
                (
                    data.syscalls().clone(),
                    data.task(),
                    data.loader_cleanup().cloned(),
                    data.abi_imports().clone(),
                )
            })
            .await;
        let graph = self.root.graph();
        let process = cx.role() == Role::Process;
        let snapshot = if process {
            let snapshot = syscalls.loader_startup(task).map_err(load_error)?;
            match snapshot {
                None if graph.libraries[0].metadata.needed().is_empty() => return Ok(None),
                None => return Err(load_error("startup policy control is unavailable")),
                Some(snapshot)
                    if graph.libraries[0].metadata.needed().is_empty()
                        && StartupPolicy::for_loader(&snapshot, self.root.policy.clone())
                            .preloads
                            .is_empty() =>
                {
                    return Ok(None);
                }
                Some(snapshot) => Some(snapshot),
            }
        } else {
            // A traditional plugin-only main has no startup fixups. Its established
            // post-bind replay path remains unchanged for custom controls.
            if graph.libraries[0].fixups.is_empty() {
                return Ok(None);
            }
            None
        };
        let cleanup = cleanup.ok_or_else(|| load_error("startup requires managed runner"))?;
        let mut pending = Pending {
            cleanup,
            owner: self.root.owner.clone(),
            closing: None,
            leases: Vec::new(),
        };
        let mut graph = if process {
            pending.closing = Some(cx.close_admission().map_err(load_error)?);
            let closing = pending.closing.as_ref().unwrap();
            syscalls
                .loader_sleep(task, closing.gate(), LoaderWait::Drained)
                .await
                .map_err(load_error)?;
            let current = syscalls
                .loader_startup(task)
                .map_err(load_error)?
                .ok_or_else(|| load_error("startup snapshot disappeared"))?;
            if !current.fresh
                || !Arc::ptr_eq(&current.gate, closing.gate())
                || !Arc::ptr_eq(&current.gate, &snapshot.unwrap().gate)
                || syscalls.loader_members(&current.gate) != Some(1)
                || !current.gate.only_store(task)
                || self.root.initialized.load(Ordering::Acquire)
            {
                return Err(load_error("startup requires a fresh single-member image"));
            }
            let main = &graph.libraries[0];
            let end = u64::from(main.base) + u64::from(main.metadata.memory_info().memory_size);
            let maximum = cx
                .with_store(|_, store| {
                    cx.imported_memory().map(|memory| {
                        memory
                            .ty(store)
                            .maximum
                            .map_or(1 << 32, |pages| u64::from(pages.0) * 65536)
                    })
                })
                .await
                .ok_or_else(|| load_error("startup requires imported memory"))?;
            let layout = MemoryLayout {
                image_end: end,
                data: u64::from(main.base)..end,
                brk: end.next_multiple_of(16),
                maximum,
                reserved: Vec::new(),
            };
            let guard = if cfg!(all(unix, feature = "sys")) {
                65536
            } else {
                0
            };
            if self.root.policy.mapped_memory() {
                cx.bootstrap(layout, 0..self.root.stack, guard).await?;
            } else {
                cx.bootstrap_linear(layout, 0..self.root.stack, guard)
                    .await?;
            }
            let memory = cx.memory();
            let caller = Caller::new(task, &memory);
            let mut graph = collect(
                &*syscalls,
                &caller,
                &graph,
                &StartupPolicy::for_loader(&current, self.root.policy.clone()),
            )
            .await
            .map_err(load_error)?;
            for lib in graph.libraries.iter_mut().skip(1) {
                let info = lib.metadata.memory_info();
                let lease = syscalls
                    .loader_reserve(&caller, info.memory_size, 1u32 << info.memory_alignment)
                    .await
                    .map_err(load_error)?;
                lib.base = lease.address();
                pending.leases.push(lease);
            }
            graph
        } else {
            (*graph).clone()
        };
        let mut tls = vec![0; graph.libraries.len()];
        if process {
            for (index, lib) in graph.libraries.iter().enumerate().skip(1) {
                tls[index] = first_tls(lib, self.root.stack).map_err(load_error)?;
            }
        } else {
            let memory = cx.memory();
            let caller = Caller::new(task, &memory);
            for (index, lib) in graph.libraries.iter().enumerate().skip(1) {
                if lib.tls_size > 0 {
                    let lease = syscalls
                        .loader_reserve(&caller, lib.tls_size, lib.tls_align)
                        .await
                        .map_err(load_error)?;
                    tls[index] = lease.address();
                    pending.leases.push(lease);
                }
            }
        }
        // Bases are now final; function slots/frozen fixups were prepared before reservations.
        cx.with_store(|_, store| self.root.prepare_code(store, &mut graph))
            .await
            .map_err(load_error)?;
        let graph = Arc::new(graph);
        let (imports, forwarders) = cx
            .with_store(|_, store| {
                main_imports(&self.root.forward_code, store, &base, &graph, &tls)
            })
            .await
            .map_err(load_error)?;
        Ok(Some(Box::new(Prepared::<S> {
            root: self.root.clone(),
            base: self.base.clone(),
            imports,
            forwarders,
            tls,
            bound: Some(Bound {
                graph,
                pending,
                env: cx.environment().clone(),
                process,
            }),
        })))
    }
}

fn first_tls(lib: &Library, stack: u32) -> Result<u32> {
    let Some(symbol) = lib.metadata.exports().get("__tls_base") else {
        return Ok(0);
    };
    let globals = initial_globals(&lib.source, infrastructure(lib, stack, 0))?;
    let end = u64::from(lib.base) + u64::from(lib.metadata.memory_info().memory_size);
    let effects = lib
        .initialization
        .effects(
            globals.clone(),
            std::slice::from_ref(&(u64::from(lib.base)..end)),
            None,
        )
        .map_err(failure)?;
    Ok(effects
        .globals
        .get(&symbol.index)
        .copied()
        .or_else(|| globals.get(&symbol.index).copied())
        .unwrap_or(0) as u32)
}

fn main_imports(
    code: &forward::Cache,
    store: &mut StoreMut<'_>,
    base: &Imports,
    graph: &Graph,
    tls: &[u32],
) -> Result<(Imports, Vec<BoundModule>)> {
    let main = &graph.libraries[0];
    let mut imports = Imports::new();
    let mut pending = Vec::new();
    for fixup in &main.fixups {
        let import = &main.metadata.imports()[fixup.import];
        match &import.ty {
            ExternType::Global(ty)
                if matches!(fixup.kind, FixupKind::GotFunction | FixupKind::GotMemory) =>
            {
                if base.get_export(&import.module, &import.name).is_some() {
                    continue;
                }
                let value = graph.target_address(&fixup.target, tls)?;
                let value = match ty.content_type {
                    wasmparser::ValType::I32 => Value::I32(value as i32),
                    _ => return Err(failure("unsupported main GOT type")),
                };
                imports.define(&import.module, &import.name, Global::new_mut(store, value));
            }
            ExternType::Function(signature) if fixup.kind == FixupKind::DirectFunction => {
                pending.push((
                    fixup.import,
                    signature.clone(),
                    graph.target_address(&fixup.target, tls)?,
                ));
            }
            _ => {}
        }
    }
    let mut forwarders = Vec::new();
    if !pending.is_empty() {
        let Some(Extern::Table(table)) = base.get_export("env", "__indirect_function_table") else {
            return Err(failure("startup requires imported function table"));
        };
        let targets = pending
            .iter()
            .map(|(_, ty, slot)| (ty.clone(), *slot))
            .collect::<Vec<_>>();
        let forwarder = forward::instantiate(code, store, &table, base, &targets)?;
        for (number, (ordinal, _, _)) in pending.iter().enumerate() {
            let import = &main.metadata.imports()[*ordinal];
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
        forwarders.push(BoundModule::new(&forwarder));
    }
    Ok((imports, forwarders))
}

async fn collect<S: wasixv2_syscalls::LoaderControl>(
    syscalls: &S,
    caller: &Caller<'_>,
    old: &Graph,
    policy: &StartupPolicy,
) -> Result<Graph> {
    let mut graph = old.clone();
    async fn open<S: wasixv2_syscalls::LoaderControl>(
        syscalls: &S,
        caller: &Caller<'_>,
        graph: &mut Graph,
        name: &[u8],
        runtime: &[String],
        origin: &[u8],
        policy: &StartupPolicy,
    ) -> Result<usize> {
        let loader = &*policy.loader;
        if loader.reuse_by_name()
            && let Some(index) = graph.libraries.iter().position(|lib| lib.request == name)
        {
            return Ok(index);
        }
        let (image, path) = load::read(
            syscalls,
            caller,
            name,
            policy.search.as_deref(),
            runtime,
            origin,
            loader,
        )
        .await?;
        if let Some(index) = graph
            .libraries
            .iter()
            .position(|lib| !loader.reuse_by_name() && lib.identity == Some(image.identity))
        {
            return Ok(index);
        }
        let mut lib = Library::new(image.image.bytes().to_vec(), Some(image.identity))?;
        lib.backing = Some(image.image);
        lib.path = path;
        lib.request = name.to_vec();
        graph.libraries.push(lib);
        Ok(graph.libraries.len() - 1)
    }
    let mut preloads = Vec::new();
    for name in &policy.preloads {
        preloads.push(
            open(
                syscalls,
                caller,
                &mut graph,
                name,
                &[],
                b"./main.wasm",
                policy,
            )
            .await?,
        );
    }
    // musl load_preload appends roots first; load_deps then starts at main,
    // loading its direct dependencies before each preload's transitive ones.
    let mut queue = VecDeque::from([0]);
    queue.extend(preloads.iter().copied());
    let mut seen = std::collections::BTreeSet::new();
    while let Some(index) = queue.pop_front() {
        if !seen.insert(index) {
            continue;
        }
        let needed = graph.libraries[index].metadata.needed().to_vec();
        let runtime = graph.libraries[index].metadata.runtime_path().to_vec();
        let origin = graph.libraries[index].path.clone();
        for name in needed {
            let dependency = open(
                syscalls,
                caller,
                &mut graph,
                name.as_bytes(),
                &runtime,
                &origin,
                policy,
            )
            .await?;
            graph.libraries[index].dependencies.push(dependency);
            queue.push_back(dependency);
        }
    }
    if policy.loader.dependency_first_handles() {
        let indices = graph.dependencies_first(1);
        for index in &mut preloads {
            *index = indices[*index];
        }
    }
    graph.global = vec![0];
    for index in preloads.iter().copied().chain(1..graph.libraries.len()) {
        if !graph.global.contains(&index) {
            graph.global.push(index);
        }
    }
    for index in 1..graph.libraries.len() {
        let info = graph.libraries[index].metadata.memory_info();
        graph.cursor = align(graph.cursor, 1u32 << info.table_alignment)?;
        graph.libraries[index].table = graph.cursor;
        graph.cursor = graph
            .cursor
            .checked_add(info.table_size)
            .ok_or_else(|| failure("startup table overflow"))?;
        graph.assign_slots(index)?;
    }
    graph.initialization.clear();
    graph.seal(0, 0)?;
    // musl load_direct_deps treats preloads as main pseudo-dependencies. Keep
    // real DT_NEEDED edges intact and choose only the constructor root order here.
    graph.initialization = order(&graph, preloads.into_iter().chain([0]));
    Ok(graph)
}
fn order(graph: &Graph, roots: impl IntoIterator<Item = usize>) -> Vec<usize> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for root in roots {
        let mut stack = vec![(root, false)];
        while let Some((index, done)) = stack.pop() {
            if done {
                out.push(index);
            } else if seen.insert(index) {
                stack.push((index, true));
                stack.extend(
                    graph.libraries[index]
                        .dependencies
                        .iter()
                        .rev()
                        .map(|dep| (*dep, false)),
                );
            }
        }
    }
    out
}

impl<S: Syscalls + Control + Send + Sync + 'static> StoreImage<S> {
    fn main_aliases(&self, store: &mut StoreMut<'_>, graph: &Graph) -> Result<()> {
        let local = lock(&self.local);
        let main = &graph.libraries[0];
        let mut placements = main
            .slots
            .iter()
            .map(|(function, slot)| (*slot, *function))
            .collect::<BTreeMap<_, _>>();
        let globals = engine_globals(&local.instances[0], store)?;
        let end = u64::from(main.table) + u64::from(main.metadata.memory_info().table_size);
        for element in main
            .initialization
            .table_effects(
                &globals,
                std::slice::from_ref(&(u64::from(main.table)..end)),
            )
            .map_err(failure)?
        {
            for (offset, function) in element.functions.iter().enumerate() {
                if let Some(function) = function {
                    placements.insert(element.index + offset as u32, *function);
                }
            }
        }
        for (slot, function) in placements {
            let (module, function) = graph.function_definition(0, function)?;
            let provider = if module < local.instances.len() {
                &local.instances[module]
            } else {
                local
                    .stage
                    .as_ref()
                    .ok_or_else(|| failure("missing startup definitions"))?
                    .modules[&module]
                    .instance()
            };
            local.table.set(
                store,
                slot,
                Value::FuncRef(Some(engine_function(provider, function)?)),
            )?;
        }
        Ok(())
    }

    pub(super) async fn finish_startup(
        &self,
        entry: &Entry<'_, S>,
        mut bound: Bound<S>,
    ) -> Result<()> {
        let graph = bound.graph.clone();
        if !bound.process {
            entry
                .with_store(|_, store| self.main_prepare(store, &graph))
                .await?;
            let task = entry.with_store(|data, _| data.task()).await;
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
            let (base_imports, hooks) = entry
                .with_store(|data, _| (data.abi_imports().clone(), data.instance_hooks().cloned()))
                .await;
            Context::Entry(entry)
                .with_store(|store| {
                    let mut local = lock(&self.local);
                    let tls = local.tls.clone();
                    local.stage_modules_with_hooks(
                        store,
                        &base_imports,
                        &work,
                        tls,
                        Vec::new(),
                        task,
                        hooks.as_ref(),
                    )
                })
                .await?;
            entry
                .with_store(|_, store| {
                    self.main_aliases(store, &graph)?;
                    let mut local = lock(&self.local);
                    let table = local.table.clone();
                    local.stage.as_mut().unwrap().apply(store, &table)?;
                    let mut stage = local.stage.take().unwrap();
                    local.leases.append(&mut stage.leases);
                    local.forwarders.append(&mut stage.forwarders);
                    for module in stage.modules.into_values() {
                        local.instances.push(module.publish()?);
                    }
                    local.tls = stage.tls;
                    local.lazy_targets = stage.lazy_targets;
                    local.generation = self.root.gate()?.generation();
                    Ok::<(), RuntimeError>(())
                })
                .await?;
            return Ok(());
        }
        // Prepare all borrowed/owning cleanup handles before activation. The existing
        // coordinator owns every cancellation path from the moment a round exists.
        let (syscalls, task) = entry
            .with_store(|data, _| (data.syscalls().clone(), data.task()))
            .await;
        let store = entry.store();
        let mut ctors = VecDeque::new();
        for index in graph
            .initialization
            .iter()
            .copied()
            .filter(|index| *index != 0)
        {
            ctors.push_back(
                graph.libraries[index]
                    .metadata
                    .exports()
                    .get("__wasm_call_ctors")
                    .and_then(|export| graph.libraries[index].slots.get(&export.index))
                    .copied()
                    .unwrap_or(0),
            );
        }
        let constructors = if self.root.policy.host_constructors() {
            std::mem::take(&mut ctors).into_iter().collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let work = Arc::new(Work {
            publication: Mutex::new(Some(Publication {
                graph: graph.clone(),
                references: BTreeMap::from([(0, 1)]),
                ctors,
            })),
            previous: 1,
            first: task,
            leases: Mutex::new(Vec::new()),
            committed: AtomicBool::new(false),
            error: Mutex::new(None),
        });
        let closing = bound
            .pending
            .closing
            .take()
            .ok_or_else(|| failure("missing startup admission"))?;
        let transaction = syscalls.loader_activate(task, closing).map_err(failure)?;
        let mut coordinator = load::Coordinator {
            transaction: Some(transaction),
            cleanup: None,
            root: self.root.clone(),
            local: self.local.clone(),
            env: bound.env,
            store,
            syscalls: syscalls.clone(),
            cleanup_scope: bound.pending.cleanup.clone(),
            task,
        };
        *lock(&work.leases) = std::mem::take(&mut bound.pending.leases);
        lock(&self.root.state).work = Some(work.clone());
        let cx = Context::Entry(entry);
        let result = async {
            entry
                .with_store(|_, store| self.main_prepare(store, &graph))
                .await?;
            self.finish_phase(&cx, &coordinator).await?;
            for phase in [LoaderPhase::Prepare, LoaderPhase::Apply] {
                if phase == LoaderPhase::Apply {
                    entry
                        .with_store(|_, store| self.main_aliases(store, &graph))
                        .await?;
                }
                coordinator
                    .transaction
                    .as_ref()
                    .unwrap()
                    .advance(phase)
                    .map_err(failure)?;
                self.finish_phase(&cx, &coordinator).await?;
                if let Some(error) = lock(&work.error).clone() {
                    return Err(failure(error));
                }
            }
            lock(&self.root.state)
                .leases
                .try_reserve(lock(&work.leases).len())
                .map_err(failure)?;
            let publication = lock(&work.publication)
                .take()
                .ok_or_else(|| failure("missing startup publication"))?;
            {
                let mut state = lock(&self.root.state);
                state.graph = publication.graph;
                state.references = publication.references;
                state.ctors = publication.ctors;
                work.committed.store(true, Ordering::Release);
            }
            coordinator
                .transaction
                .as_ref()
                .unwrap()
                .advance(LoaderPhase::Publish)
                .map_err(failure)?;
            self.finish_phase(&cx, &coordinator).await?;
            lock(&self.root.state)
                .leases
                .extend(std::mem::take(&mut *lock(&work.leases)));
            lock(&self.root.state).work = None;
            coordinator
                .transaction
                .take()
                .unwrap()
                .finish()
                .map_err(failure)?;
            Ok(())
        }
        .await;
        if let Err(error) = result {
            let _ = self.root.error.set(format!("startup failed: {error}"));
            if work.committed.load(Ordering::Acquire) {
                let _ = syscalls.loader_fatal(self.root.gate()?);
                drop(coordinator);
            } else {
                coordinator.abort().await?;
            }
            return Err(error);
        }
        self.call_constructors(&Context::Entry(entry), &constructors)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_policy_ignores_secure_environment_and_preserves_first_value_order() {
        let mut startup = LoaderStartup {
            gate: LoaderGate::new(1),
            environment: vec![
                b"LD_LIBRARY_PATH=/first::/last".to_vec(),
                b"LD_LIBRARY_PATH=/ignored".to_vec(),
                b"LD_PRELOAD= :a.so\tb.so::a.so\n".to_vec(),
            ],
            secure: false,
            fresh: true,
        };
        let policy = StartupPolicy::new(&startup);
        assert_eq!(
            policy.search.as_deref(),
            Some(&b"/first::/last:/lib:/usr/local/lib:/usr/lib"[..])
        );
        assert_eq!(
            policy.preloads,
            [b"a.so".to_vec(), b"b.so".to_vec(), b"a.so".to_vec()]
        );
        startup.secure = true;
        let policy = StartupPolicy::new(&startup);
        assert_eq!(
            policy.search.as_deref(),
            Some(&b"/lib:/usr/local/lib:/usr/lib"[..])
        );
        assert!(policy.preloads.is_empty());
    }
    #[test]
    fn main_direct_dependency_precedes_transitive_preload_symbol() {
        use std::{
            future::Future,
            task::{Context, Poll, Waker},
        };
        use wasixv2_syscalls::{Fault, LibraryImage, LoaderControl, UserMemory};
        struct Files(BTreeMap<Vec<u8>, (u64, Vec<u8>)>);
        impl LoaderControl for Files {
            fn loader_read<'a>(
                &'a self,
                caller: &'a Caller<'_>,
                path: &'a [u8],
            ) -> LocalFuture<'a, SysResult<LibraryImage>> {
                Box::pin(async move {
                    assert_eq!(caller.task(), TaskId(7));
                    let name = path.rsplit(|b| *b == b'/').next().unwrap();
                    let (inode, bytes) = self.0.get(name).ok_or(Errno::ENOENT)?;
                    Ok(LibraryImage {
                        path: None,
                        identity: (1, *inode),
                        image: wasixv2_syscalls::Image::copied(bytes.clone()),
                    })
                })
            }
        }
        struct Memory;
        impl UserMemory for Memory {
            fn size(&self) -> LocalFuture<'_, u64> {
                Box::pin(async { 0 })
            }
            fn grow(&self, _: u32) -> LocalFuture<'_, Option<u64>> {
                Box::pin(async { None })
            }
            fn read<'a>(
                &'a self,
                _: u32,
                _: &'a mut [u8],
            ) -> LocalFuture<'a, std::result::Result<(), Fault>> {
                Box::pin(async { Err(Fault) })
            }
            fn write<'a>(
                &'a self,
                _: u32,
                _: &'a [u8],
            ) -> LocalFuture<'a, std::result::Result<(), Fault>> {
                Box::pin(async { Err(Fault) })
            }
        }
        let bytes = |wat, needed: &[&str]| crate::tests::bytes(wat, [0; 4], needed, &[], &[]);
        let root = Root::new(bytes(
            r#"(module (import "env" "choice" (func (result i32))))"#,
            &["main-dep.so"],
        ));
        let files = Files(BTreeMap::from([
            (
                b"preload.so".to_vec(),
                (
                    1,
                    bytes(
                        r#"(module (func (export "preload_only") (result i32) i32.const 31))"#,
                        &["preload-dep.so"],
                    ),
                ),
            ),
            (
                b"main-dep.so".to_vec(),
                (
                    2,
                    bytes(
                        r#"(module (func (export "choice") (result i32) i32.const 11))"#,
                        &[],
                    ),
                ),
            ),
            (
                b"preload-dep.so".to_vec(),
                (
                    3,
                    bytes(
                        r#"(module (func (export "choice") (result i32) i32.const 22))"#,
                        &[],
                    ),
                ),
            ),
        ]));
        let memory = Memory;
        let caller = Caller::new(TaskId(7), &memory);
        let old = root.graph();
        let policy = StartupPolicy {
            loader: Arc::new(policy::NativePolicy),
            search: Some(b"/lib".to_vec()),
            preloads: vec![b"preload.so".to_vec()],
        };
        let mut future = Box::pin(collect(&files, &caller, &old, &policy));
        let graph = match future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Ready(Ok(graph)) => graph,
            _ => panic!("in-memory graph must resolve"),
        };
        let main_dep = graph
            .libraries
            .iter()
            .position(|lib| lib.identity == Some((1, 2)))
            .unwrap();
        assert!(
            matches!(graph.libraries[0].fixups[0].target, BindingTarget::Function { module, .. } if module == main_dep),
            "main's direct dependency must win the competing symbol"
        );
        assert_eq!(
            graph
                .global
                .iter()
                .map(|index| graph.libraries[*index].identity)
                .collect::<Vec<_>>(),
            [None, Some((1, 1)), Some((1, 2)), Some((1, 3))]
        );
        assert_eq!(
            graph.symbol(0, "preload_only", &[0; 4]).unwrap(),
            graph.symbol(1, "preload_only", &[0; 4]).unwrap()
        );
        assert_eq!(graph.initialization, [3, 1, 2, 0]);
    }

    #[test]
    fn preload_constructor_roots_do_not_forge_needed_edges_or_repeat_cycles() {
        let library =
            || Library::new(crate::tests::bytes("(module)", [0; 4], &[], &[], &[]), None).unwrap();
        let mut graph = Graph {
            host_imports: None,
            libraries: vec![library(), library(), library(), library()],
            global: vec![0, 1, 2, 3],
            cursor: 3,
            initialization: Vec::new(),
        };
        graph.libraries[0].dependencies = vec![2];
        graph.libraries[1].dependencies = vec![3];
        graph.libraries[2].dependencies = vec![3];
        graph.libraries[3].dependencies = vec![1];
        assert_eq!(order(&graph, [1, 0]), [3, 1, 2, 0]);
        assert_eq!(graph.libraries[0].dependencies, [2]);
    }
}
