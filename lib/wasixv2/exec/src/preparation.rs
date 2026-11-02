//! Host-only preparation of imports before the main instance exists.
//!
//! For exec this follows the kernel's commit (fs/exec.c:begin_new_exec), not format
//! probing: an error uses the existing new-image failure path, never the old guest.

use std::{cell::RefCell, ops::Range, sync::Arc};

use wasixv2_syscalls::{Control, Errno, LoaderClosing, MemoryLayout, Syscalls, UserMemory};
use wasmer::{FunctionEnv, Memory, Module, Store};

use crate::{
    Abi, Binding, ExecEnv, ExecError, LoadError, PreparedImage, Role,
    asyncify::{Access, Outside},
    memory::{self, OutsideMemory},
};

/// One owning store's pre-instantiation context. It cannot enter guest code.
/// The returned prepared attachment must own its store-local state, without retaining
/// this borrow or an async store handle beyond the preparation call.
pub struct Preparation<'a, S: 'static> {
    cx: Outside<'a, S>,
    module: &'a Module,
    role: Role,
    imported: Option<&'a Memory>,
    binding: &'a RefCell<Option<Binding>>,
    bootstrap: RefCell<Option<Bootstrap>>,
}

#[derive(Clone)]
pub(crate) struct Bootstrap {
    pub(crate) layout: Option<MemoryLayout>,
    pub(crate) stack: Range<u32>,
    pub(crate) guard: u32,
}

impl<S: Syscalls + Control + Send + Sync + 'static> Preparation<'_, S> {
    pub fn module(&self) -> &Module {
        self.module
    }

    pub fn role(&self) -> Role {
        self.role
    }

    /// A store-affine environment handle for the prepared attachment, without
    /// retaining the async store or permitting guest calls from this context.
    pub fn environment(&self) -> &FunctionEnv<ExecEnv<S>> {
        self.cx.env
    }

    /// The actual memory provided to this module's memory import, if it has one.
    pub fn imported_memory(&self) -> Option<&Memory> {
        self.imported
    }

    /// The real new memory after bootstrap, or the inherited memory of a thread.
    pub fn memory(&self) -> impl UserMemory + '_ {
        OutsideMemory { cx: &self.cx }
    }

    /// A short host-only operation; never hold this store lock across another await.
    pub async fn with_store<R>(
        &self,
        f: impl FnOnce(&mut ExecEnv<S>, &mut wasmer::StoreMut<'_>) -> R,
    ) -> R {
        self.cx.with(f).await
    }

    /// Closes the existing gate and hands off only this binding's temporary admission.
    /// Store registration and the selected image owner remain live through binding.
    pub fn close_admission(&self) -> Result<LoaderClosing, Errno> {
        let mut binding = self.binding.borrow_mut();
        let binding = binding.as_mut().ok_or(Errno::ENOSYS)?;
        if binding.admission.is_none() {
            return Err(Errno::EBUSY);
        }
        let closing = binding.gate.close(binding.task)?;
        drop(binding.admission.take());
        Ok(closing)
    }

    /// Attaches the actual new shared memory before native startup reservations.
    /// The image owner must first validate its trusted fresh-mm startup snapshot,
    /// close/drain admission and validate the source-derived layout. Threads inherit
    /// their existing attachment and cannot bootstrap another one.
    pub async fn bootstrap(
        &self,
        layout: MemoryLayout,
        stack: Range<u32>,
        guard: u32,
    ) -> Result<(), ExecError> {
        self.bootstrap_profile(layout, stack, guard, true).await
    }

    /// Attaches a source-bounded image whose ABI manages a linear `memory.grow`
    /// heap, as an ordinary ABI with `image_layout = None` does. Admission,
    /// imported-memory identity, extent and guard validation are identical.
    pub async fn bootstrap_linear(
        &self,
        layout: MemoryLayout,
        stack: Range<u32>,
        guard: u32,
    ) -> Result<(), ExecError> {
        self.bootstrap_profile(layout, stack, guard, false).await
    }

    async fn bootstrap_profile(
        &self,
        layout: MemoryLayout,
        stack: Range<u32>,
        guard: u32,
        mapped: bool,
    ) -> Result<(), ExecError> {
        let bad = || ExecError::Load(LoadError::Format(Errno::EINVAL));
        if self.role != Role::Process || self.bootstrap.borrow().is_some() {
            return Err(bad());
        }
        {
            let binding = self.binding.borrow();
            let binding = binding.as_ref().ok_or_else(bad)?;
            if binding.admission.is_some() || !binding.gate.only_store(binding.task) {
                return Err(bad());
            }
        }
        let memory = self.imported.ok_or_else(bad)?.clone();
        self.with_store(|env, store| {
            if env.memory.is_some()
                || layout.data.start > layout.data.end
                || layout.data.end > layout.image_end
                || layout.image_end > layout.brk
                || layout.brk > layout.maximum
                || stack.start > stack.end
                || u64::from(stack.end) > layout.image_end
                || u64::from(guard) > u64::from(stack.end)
                || !matches!(guard, 0 | memory::LOW_GUARD)
                || layout.maximum > 1 << 32
                || !layout
                    .maximum
                    .is_multiple_of(wasixv2_syscalls::WASM_PAGE_SIZE)
                || layout.reserved.iter().any(|range| {
                    range.start > range.end
                        || range.end > layout.maximum
                        || !range.start.is_multiple_of(wasixv2_syscalls::WASM_PAGE_SIZE)
                        || !range.end.is_multiple_of(wasixv2_syscalls::WASM_PAGE_SIZE)
                })
            {
                return Err(bad());
            }
            let maximum = memory.ty(&*store).maximum.map_or(1 << 32, |pages| {
                u64::from(pages.0) * wasixv2_syscalls::WASM_PAGE_SIZE
            });
            if layout.maximum != maximum {
                return Err(bad());
            }
            let size = memory.view(&*store).data_size();
            if layout.image_end > size {
                let pages = (layout.image_end - size).div_ceil(wasixv2_syscalls::WASM_PAGE_SIZE);
                memory.grow(&mut *store, u32::try_from(pages).map_err(|_| bad())?)?;
            }
            let shared = memory.as_shared(&*store).ok_or_else(bad)?;
            let foreign = memory::ForeignMemory::new(
                shared,
                guard,
                env.read_only.clone(),
                env.page_access.clone(),
            );
            env.memory = Some(memory.clone());
            env.layout = mapped.then(|| layout.clone());
            env.main_stack = stack.clone();
            env.low_guard = guard;
            env.syscalls.attach_memory(env.task, Arc::new(foreign));
            Ok(())
        })
        .await?;
        self.bootstrap.replace(Some(Bootstrap {
            layout: mapped.then_some(layout),
            stack,
            guard,
        }));
        Ok(())
    }
}

pub(crate) async fn prepare<S>(
    store: Store,
    env: &FunctionEnv<ExecEnv<S>>,
    module: &Module,
    role: Role,
    abi: &dyn Abi<S>,
    imported: Option<&Memory>,
    binding: &mut Option<Binding>,
) -> Result<(Store, Option<Box<dyn PreparedImage<S>>>, Option<Bootstrap>), ExecError>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    let hooks = env.as_ref(&store).instance_hooks().cloned();
    let scope = env.as_ref(&store).loader_cleanup().cloned();
    let store = store.into_async();
    let mut attachments = hooks
        .clone()
        .zip(scope.clone())
        .map(|(hooks, scope)| hooks.guard(&store, scope));
    let binding_cell = RefCell::new(binding.take());
    let preparation = Preparation {
        cx: Outside { env, store: &store },
        module,
        role,
        imported,
        binding: &binding_cell,
        bootstrap: RefCell::new(None),
    };
    let prepared = abi.prepare(&preparation).await?;
    let bootstrap = preparation.bootstrap.into_inner();
    if bootstrap.is_some() && prepared.is_none() {
        return Err(ExecError::Load(LoadError::Format(Errno::EINVAL)));
    }
    *binding = binding_cell.into_inner();
    if let Some(attachments) = &mut attachments {
        attachments.disarm();
    }
    drop(attachments);
    let store = match store.into_store() {
        Ok(store) => store,
        Err(store) => {
            if let Some((hooks, scope)) = hooks.zip(scope) {
                let mut attachments = hooks.guard(&store, scope);
                attachments.deactivate().await;
            }
            return Err(ExecError::Load(LoadError::Format(Errno::EBUSY)));
        }
    };
    Ok((store, prepared, bootstrap))
}

pub(crate) fn verify_memory(
    module: &Module,
    imports: &wasmer::Imports,
    imported: Option<&Memory>,
) -> Result<(), ExecError> {
    if let Some(import) = module.imports().memories().next()
        && !matches!(
            imports.get_export(import.module(), import.name()),
            Some(wasmer::Extern::Memory(memory)) if Some(&memory) == imported
        )
    {
        return Err(ExecError::Load(LoadError::Format(Errno::EINVAL)));
    }
    Ok(())
}

#[cfg(all(test, feature = "sys"))]
pub(crate) mod tests {
    use super::*;
    use crate::{
        AbiInstance, Abis, Entry, EntryReturn, Sigreturn, VforkResume, Wasixv2,
        process::tests::{NoThreads, Recorder},
    };
    use std::{
        future::Future,
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        task::{Context, Waker},
    };
    use wasixv2_syscalls::{LocalFuture, SignalFrame, TaskId};
    use wasmer::{AsyncFunctionEnvMut, Function, Imports, Instance, RuntimeError};

    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Mode {
        Prepared,
        Pending,
        BindFailure,
        WrongMemory,
        LayoutMismatch,
        NoneAfterBootstrap,
    }

    #[derive(Default)]
    pub(crate) struct Probe {
        pub(crate) claims: AtomicUsize,
        pub(crate) ordinary_binds: AtomicUsize,
        pub(crate) prepared_binds: AtomicUsize,
        pub(crate) entries: AtomicUsize,
        pub(crate) guest_calls: AtomicUsize,
        pub(crate) drops: AtomicUsize,
        pub(crate) roles: Mutex<Vec<Role>>,
        env: Mutex<Option<FunctionEnv<ExecEnv<Recorder>>>>,
        owner: Arc<()>,
    }
    pub(crate) struct ProbeAbi {
        pub(crate) probe: Arc<Probe>,
        pub(crate) mode: Mode,
    }
    pub(crate) fn layout() -> MemoryLayout {
        MemoryLayout {
            image_end: 196608,
            data: 131072..196608,
            brk: 196608,
            maximum: 8 * 65536,
            reserved: Vec::new(),
        }
    }
    #[allow(clippy::field_reassign_with_default)] // other Recorder test fields are private
    pub(crate) fn control(mm: u64) -> Arc<Recorder> {
        let mut recorder = Recorder::default();
        recorder.loader = Some(wasixv2_syscalls::LoaderGate::new(mm));
        Arc::new(recorder)
    }
    pub(crate) fn module(store: &Store) -> Module {
        Module::new(
            store,
            wat::parse_str(
                r#"(module
            (import "env" "memory" (memory 1 8 shared))
            (import "wasixv2" "exit_group" (func $exit (param i32)))
            (import "proof" "value" (func $value (result i32)))
            (export "memory" (memory 0))
            (func (export "_start") call $value call $exit)
            (func (export "wasixv2_thread_start") (param i32 i32) call $value drop i32.const 0 call $exit))"#,
            )
            .unwrap(),
        )
        .unwrap()
    }
    impl Abi<Recorder> for ProbeAbi {
        fn name(&self) -> &'static str {
            "prepared-proof"
        }
        fn matches(&self, _: &Module) -> bool {
            true
        }
        fn matches_image(&self, _: &[u8]) -> bool {
            true
        }
        fn claim_image(
            &self,
            gate: &Arc<wasixv2_syscalls::LoaderGate>,
        ) -> Result<wasixv2_syscalls::LoaderAdmission, Errno> {
            self.probe.claims.fetch_add(1, Ordering::Relaxed);
            gate.claim_image(&self.probe.owner)
        }
        fn imports(
            &self,
            store: &mut Store,
            env: &FunctionEnv<ExecEnv<Recorder>>,
            module: &Module,
        ) -> Imports {
            *self.probe.env.lock().unwrap() = Some(env.clone());
            let mut imports = <Wasixv2 as Abi<Recorder>>::imports(&Wasixv2, store, env, module);
            imports.define("proof", "value", Function::new_typed(store, || 3i32));
            imports
        }
        fn prepare<'a>(
            &'a self,
            cx: &'a Preparation<'a, Recorder>,
        ) -> LocalFuture<'a, Result<Option<Box<dyn PreparedImage<Recorder>>>, ExecError>> {
            Box::pin(async move {
                self.probe.roles.lock().unwrap().push(cx.role());
                assert_eq!(self.probe.guest_calls.load(Ordering::Relaxed), 0);
                let closing = if cx.role() == Role::Process {
                    assert_eq!(cx.memory().size().await, 0);
                    let closing = cx.close_admission().unwrap();
                    assert!(
                        closing
                            .gate()
                            .ready_for(wasixv2_syscalls::LoaderWait::Drained),
                        "handoff must release its own admission before awaiting drain"
                    );
                    closing.drained().await;
                    assert!(closing.gate().only_store(TaskId(1)));
                    assert!(
                        closing.gate().bind(TaskId(1)).is_err(),
                        "registered while admission is handed off"
                    );
                    cx.bootstrap(layout(), 0..131072, 0).await?;
                    Some(closing)
                } else {
                    assert_eq!(cx.memory().layout().await, Some(layout()));
                    None
                };
                assert_eq!(cx.memory().layout().await, Some(layout()));
                assert_eq!(cx.memory().main_stack().await, 0..131072);
                assert!(cx.memory().size().await >= layout().image_end);
                cx.memory()
                    .write(131080, &91u32.to_le_bytes())
                    .await
                    .unwrap();
                cx.with_store(|env, store| {
                    let mut bytes = [0; 4];
                    cx.imported_memory()
                        .unwrap()
                        .view(&*store)
                        .read(131080, &mut bytes)
                        .unwrap();
                    assert_eq!(
                        u32::from_le_bytes(bytes),
                        91,
                        "bootstrap uses the actual imported memory"
                    );
                    assert!(env.imports().get_export("proof", "value").is_some());
                })
                .await;
                if self.mode == Mode::Pending {
                    std::future::pending::<()>().await;
                }
                if self.mode == Mode::NoneAfterBootstrap {
                    return Ok(None);
                }
                let probe = self.probe.clone();
                let imports = cx
                    .with_store(|_, store| {
                        let mut imports = Imports::new();
                        imports.define(
                            "proof",
                            "value",
                            Function::new_typed(&mut *store, move || {
                                probe.guest_calls.fetch_add(1, Ordering::Relaxed);
                                17i32
                            }),
                        );
                        if self.mode == Mode::WrongMemory {
                            imports.define(
                                "env",
                                "memory",
                                Memory::new(store, wasmer::MemoryType::new(1, Some(8), true))
                                    .unwrap(),
                            );
                        }
                        imports
                    })
                    .await;
                Ok(Some(Box::new(Attachment {
                    imports,
                    closing,
                    probe: self.probe.clone(),
                    mode: self.mode,
                }) as Box<dyn PreparedImage<Recorder>>))
            })
        }
        fn bind(
            &self,
            instance: &Instance,
            store: &mut Store,
            task: TaskId,
            role: Role,
        ) -> Result<Arc<dyn AbiInstance<Recorder>>, ExecError> {
            self.probe.ordinary_binds.fetch_add(1, Ordering::Relaxed);
            let env = self.probe.env.lock().unwrap();
            let data = env.as_ref().unwrap().as_ref(store);
            assert!(
                data.memory().is_none(),
                "None retains ordinary pre-bind env"
            );
            assert_eq!(data.imports().into_iter().count(), 0);
            <Wasixv2 as Abi<Recorder>>::bind(&Wasixv2, instance, store, task, role)
        }
        fn image_layout(
            &self,
            _: &Instance,
            _: &Memory,
            _: &mut Store,
            _: bool,
        ) -> Option<MemoryLayout> {
            let mut layout = layout();
            if self.mode == Mode::LayoutMismatch {
                layout.brk += 16;
            }
            Some(layout)
        }
        fn main_stack(&self, _: &Instance, _: &mut Store) -> Range<u32> {
            0..131072
        }
    }
    /// Deliberately omits prepare, exercising the unchanged default contract.
    struct NoPreparation(ProbeAbi);
    impl Abi<Recorder> for NoPreparation {
        fn name(&self) -> &'static str {
            "no-preparation"
        }
        fn matches(&self, _: &Module) -> bool {
            true
        }
        fn matches_image(&self, _: &[u8]) -> bool {
            true
        }
        fn claim_image(
            &self,
            gate: &Arc<wasixv2_syscalls::LoaderGate>,
        ) -> Result<wasixv2_syscalls::LoaderAdmission, Errno> {
            self.0.claim_image(gate)
        }
        fn imports(
            &self,
            store: &mut Store,
            env: &FunctionEnv<ExecEnv<Recorder>>,
            module: &Module,
        ) -> Imports {
            self.0.imports(store, env, module)
        }
        fn bind(
            &self,
            instance: &Instance,
            store: &mut Store,
            task: TaskId,
            role: Role,
        ) -> Result<Arc<dyn AbiInstance<Recorder>>, ExecError> {
            self.0.bind(instance, store, task, role)
        }
    }
    #[test]
    fn default_none_preserves_custom_imports_bind_environment_and_admission() {
        let store = Store::default();
        let module = module(&store);
        let probe = Arc::<Probe>::default();
        let recorder = control(82);
        let mut abis = Abis::new();
        abis.push(Arc::new(NoPreparation(ProbeAbi {
            probe: probe.clone(),
            mode: Mode::Prepared,
        })));
        let ready = futures::executor::block_on(crate::instantiate(
            store,
            &module,
            recorder,
            TaskId(1),
            Arc::new(NoThreads),
            Arc::new(abis),
            wasixv2_host::LoaderCleanup::new(Arc::new(NoThreads)),
            None,
        ))
        .unwrap();
        assert_eq!(probe.ordinary_binds.load(Ordering::Relaxed), 1);
        assert_eq!(probe.prepared_binds.load(Ordering::Relaxed), 0);
        assert_eq!(probe.claims.load(Ordering::Relaxed), 1);
        let ended = futures::executor::block_on(ready.start());
        assert!(matches!(ended, crate::Ended::Exited(3)));
        assert_eq!(probe.claims.load(Ordering::Relaxed), 1);
        assert!(probe.roles.lock().unwrap().is_empty());
    }
    struct Attachment {
        imports: Imports,
        closing: Option<LoaderClosing>,
        probe: Arc<Probe>,
        mode: Mode,
    }
    impl Drop for Attachment {
        fn drop(&mut self) {
            self.probe.drops.fetch_add(1, Ordering::Relaxed);
        }
    }
    impl PreparedImage<Recorder> for Attachment {
        fn imports(&self) -> &Imports {
            &self.imports
        }
        fn bind(
            mut self: Box<Self>,
            instance: &Instance,
            store: &mut Store,
            task: TaskId,
            role: Role,
        ) -> Result<Arc<dyn AbiInstance<Recorder>>, ExecError> {
            self.probe.prepared_binds.fetch_add(1, Ordering::Relaxed);
            assert_eq!(self.probe.guest_calls.load(Ordering::Relaxed), 0);
            if self.mode == Mode::BindFailure {
                return Err(ExecError::Load(LoadError::Format(Errno::ENOEXEC)));
            }
            Ok(Arc::new(PreparedInstance {
                inner: <Wasixv2 as Abi<Recorder>>::bind(&Wasixv2, instance, store, task, role)?,
                closing: Mutex::new(self.closing.take()),
                probe: self.probe.clone(),
            }))
        }
    }
    struct PreparedInstance {
        inner: Arc<dyn AbiInstance<Recorder>>,
        closing: Mutex<Option<LoaderClosing>>,
        probe: Arc<Probe>,
    }
    impl AbiInstance<Recorder> for PreparedInstance {
        fn prepare_entry<'a>(
            &'a self,
            entry: &'a Entry<'a, Recorder>,
        ) -> LocalFuture<'a, Result<(), RuntimeError>> {
            Box::pin(async move {
                // No guest call has installed StoreContext yet. The direct owned
                // Entry handle still reaches this store for finite host cleanup.
                let handle = entry.store();
                let memory = entry
                    .with_store(|env, _| env.memory().unwrap().clone())
                    .await;
                let mut bytes = [0; 4];
                memory
                    .view(&handle.read())
                    .read(131080, &mut bytes)
                    .unwrap();
                assert_eq!(u32::from_le_bytes(bytes), 91);
                drop(handle);
                if let Some(closing) = self.closing.lock().unwrap().take() {
                    assert!(closing.gate().busy());
                    drop(closing);
                }
                self.probe.entries.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })
        }
        fn start<'a>(
            &'a self,
            entry: &'a Entry<'a, Recorder>,
        ) -> LocalFuture<'a, Result<(), RuntimeError>> {
            self.inner.start(entry)
        }
        fn on_entry_return(&self) -> EntryReturn {
            self.inner.on_entry_return()
        }
        fn start_thread<'a>(
            &'a self,
            entry: &'a Entry<'a, Recorder>,
            tid: i32,
            arg: u32,
        ) -> LocalFuture<'a, Result<(), RuntimeError>> {
            self.inner.start_thread(entry, tid, arg)
        }
        fn on_thread_return(&self) -> EntryReturn {
            self.inner.on_thread_return()
        }
        fn deliver_signal<'a>(
            &'a self,
            env: &'a AsyncFunctionEnvMut<ExecEnv<Recorder>>,
            frame: &'a SignalFrame,
            restart: bool,
        ) -> LocalFuture<'a, Result<Sigreturn, RuntimeError>> {
            self.inner.deliver_signal(env, frame, restart)
        }
        fn vfork_resume<'a>(
            &'a self,
            env: &'a AsyncFunctionEnvMut<ExecEnv<Recorder>>,
            child: TaskId,
            pid: i32,
        ) -> LocalFuture<'a, VforkResume> {
            self.inner.vfork_resume(env, child, pid)
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }
    fn setup(
        mode: Mode,
    ) -> (
        Store,
        Module,
        Arc<Recorder>,
        Arc<Probe>,
        Arc<Abis<Recorder>>,
    ) {
        let store = Store::default();
        let module = module(&store);
        let recorder = control(81);
        let probe = Arc::<Probe>::default();
        let mut abis = Abis::new();
        abis.push(Arc::new(ProbeAbi {
            probe: probe.clone(),
            mode,
        }));
        (store, module, recorder, probe, Arc::new(abis))
    }
    #[test]
    fn real_memory_preparation_supplies_imports_and_consumes_bind_before_entry() {
        let (store, module, recorder, probe, abis) = setup(Mode::Prepared);
        let ready = futures::executor::block_on(crate::instantiate(
            store,
            &module,
            recorder.clone(),
            TaskId(1),
            Arc::new(NoThreads),
            abis,
            wasixv2_host::LoaderCleanup::new(Arc::new(NoThreads)),
            None,
        ))
        .unwrap();
        assert_eq!(probe.prepared_binds.load(Ordering::Relaxed), 1);
        assert_eq!(probe.ordinary_binds.load(Ordering::Relaxed), 0);
        assert_eq!(probe.guest_calls.load(Ordering::Relaxed), 0);
        assert_eq!(probe.claims.load(Ordering::Relaxed), 1);
        let gate = recorder.loader.as_ref().unwrap();
        assert!(gate.busy());
        let ended = futures::executor::block_on(ready.start());
        assert!(matches!(ended, crate::Ended::Exited(17)));
        assert_eq!(probe.entries.load(Ordering::Relaxed), 1);
        assert_eq!(
            probe.claims.load(Ordering::Relaxed),
            2,
            "reclaim admission without duplicate bind"
        );
        assert_eq!(probe.guest_calls.load(Ordering::Relaxed), 1);
        assert!(!gate.busy());
        let admitted = gate.claim_image(&probe.owner).unwrap();
        admitted.bind(TaskId(1)).unwrap();
        gate.leave(TaskId(1));
    }
    #[test]
    fn failed_prepared_bind_and_invalid_memory_or_layout_release_frame_and_closing() {
        for mode in [
            Mode::BindFailure,
            Mode::WrongMemory,
            Mode::LayoutMismatch,
            Mode::NoneAfterBootstrap,
        ] {
            let (store, module, recorder, probe, abis) = setup(mode);
            let result = futures::executor::block_on(crate::instantiate(
                store,
                &module,
                recorder.clone(),
                TaskId(1),
                Arc::new(NoThreads),
                abis,
                wasixv2_host::LoaderCleanup::new(Arc::new(NoThreads)),
                None,
            ));
            assert!(result.is_err());
            assert_eq!(probe.guest_calls.load(Ordering::Relaxed), 0);
            let gate = recorder.loader.as_ref().unwrap();
            assert!(!gate.busy());
            let admitted = gate.claim_image(&probe.owner).unwrap();
            admitted.bind(TaskId(1)).unwrap();
            gate.leave(TaskId(1));
        }
    }
    #[test]
    fn canceled_preparation_reopens_admission_and_leaves_registered_store() {
        let (store, module, recorder, probe, abis) = setup(Mode::Pending);
        let mut future = Box::pin(crate::instantiate(
            store,
            &module,
            recorder.clone(),
            TaskId(1),
            Arc::new(NoThreads),
            abis,
            wasixv2_host::LoaderCleanup::new(Arc::new(NoThreads)),
            None,
        ));
        assert!(
            future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        let gate = recorder.loader.as_ref().unwrap();
        assert!(gate.busy());
        assert!(gate.only_store(TaskId(1)));
        drop(future);
        assert!(!gate.busy());
        let admitted = gate.claim_image(&probe.owner).unwrap();
        admitted.bind(TaskId(1)).unwrap();
        gate.leave(TaskId(1));
        assert_eq!(probe.guest_calls.load(Ordering::Relaxed), 0);
    }
}
