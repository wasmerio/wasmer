//! Guest threads. A thread created with a shared address space is another instance of the
//! process's module on the same shared memory; the host's executor decides where it runs. So
//! is the first task of a process that shares its creator's memory (`thread-spawn` without
//! `CLONE_THREAD`), whose instance runs as that process until it exits or its `execve` starts
//! the new image on an instance of its own.
//!
//! The kernel creates the task in `thread-spawn` and asks for the start on the return-to-guest
//! path ([`GuestAction::StartThread`]), where the calling instance's [`ExecEnv`] holds the image
//! its threads instantiate.
//!
//! [`GuestAction::StartThread`]: wasixv2_syscalls::GuestAction::StartThread

use std::{
    io,
    ops::Range,
    sync::{Arc, Mutex, PoisonError},
};

use wasixv2_abi::Signal;
use wasixv2_host::Host;
use wasixv2_syscalls::{Control, Syscalls, TaskId};
use wasmer::{AsyncFunctionEnvMut, Engine, FunctionEnv, Instance, Module, SharedMemory, Store};

use crate::{
    Ended, ExecEnv, ExecError,
    abi::{Abi, Abis, Entry, Role},
    asyncify::Access,
    base::Base,
    memory::ReadOnly,
    process,
};

// The permit is created only by a ThreadStart factory; ambient TLS alone never
// authorizes the public prebuilt-Store entry point.
#[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
thread_local! {
    static OWNER_THREAD: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
    static NEXT_OWNER: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

pub(crate) struct OwningThread {
    lease: OwnerLease,
    retirement: Arc<process::Handoff>,
    #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
    permit: OwnerPermit,
}

struct OwnerLease {
    retired: Option<process::Takeover>,
    #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
    id: u64,
    #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
    thread: std::thread::ThreadId,
    _local: std::marker::PhantomData<*mut ()>,
}

#[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
struct OwnerPermit {
    #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
    id: u64,
    #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
    thread: std::thread::ThreadId,
    _local: std::marker::PhantomData<*mut ()>,
}

impl OwningThread {
    pub(crate) fn acquire() -> Result<Self, ExecError> {
        #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
        {
            if OWNER_THREAD.with(|owner| owner.get().is_some()) {
                return Err(ExecError::OwningThreadBusy);
            }
            let id = NEXT_OWNER.with(|next| -> Result<u64, ExecError> {
                let id = next
                    .get()
                    .checked_add(1)
                    .ok_or(ExecError::OwningThreadBusy)?;
                next.set(id);
                Ok(id)
            })?;
            let thread = std::thread::current().id();
            let retirement = Arc::new(process::Handoff::default());
            OWNER_THREAD.with(|owner| owner.set(Some(id)));
            Ok(Self {
                lease: OwnerLease {
                    retired: Some(process::Takeover(retirement.clone())),
                    id,
                    thread,
                    _local: std::marker::PhantomData,
                },
                retirement,
                permit: OwnerPermit {
                    id,
                    thread,
                    _local: std::marker::PhantomData,
                },
            })
        }
        #[cfg(not(all(feature = "sys", not(target_arch = "wasm32"))))]
        {
            Err(ExecError::OwningThreadUnsupported)
        }
    }

    pub(crate) fn required(required: bool) -> Result<Option<Self>, ExecError> {
        required.then(Self::acquire).transpose()
    }

    pub(crate) fn authorize(&self) -> Result<(), ExecError> {
        #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
        if self.permit.thread == std::thread::current().id()
            && OWNER_THREAD.with(|owner| owner.get() == Some(self.permit.id))
        {
            return Ok(());
        }
        Err(ExecError::OwningThreadRequired)
    }

    pub(crate) async fn run<R>(
        self,
        cleanup: &Arc<wasixv2_host::LoaderCleanup>,
        guest: impl std::future::Future<Output = R>,
    ) -> R {
        use std::{
            future::{Future, poll_fn},
            panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
            task::Poll,
        };
        self.authorize().expect("private owning-thread permit");
        let mut runner = Some(Box::pin(cleanup.run_with_keepalive(guest, self.lease)));
        let result = poll_fn(|cx| {
            match catch_unwind(AssertUnwindSafe(|| {
                runner.as_mut().unwrap().as_mut().poll(cx)
            })) {
                Ok(Poll::Pending) => Poll::Pending,
                Ok(Poll::Ready(value)) => Poll::Ready(Ok(value)),
                Err(panic) => Poll::Ready(Err(panic)),
            }
        })
        .await;
        // Runner Drop can transfer its drain even if guest/output destruction
        // panics. Keep this worker alive until that exact drain releases its lease.
        let dropped = catch_unwind(AssertUnwindSafe(|| drop(runner.take())));
        self.retirement
            .wait()
            .await
            .expect("owning-thread lease retirement");
        match (result, dropped) {
            (Ok(value), Ok(())) => value,
            (Err(panic), Ok(())) => resume_unwind(panic),
            (Ok(value), Err(panic)) => {
                drop(value);
                resume_unwind(panic)
            }
            (Err(first), Err(second)) => {
                drop(second);
                resume_unwind(first)
            }
        }
    }
}

impl Drop for OwnerLease {
    fn drop(&mut self) {
        #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
        {
            assert_eq!(
                self.thread,
                std::thread::current().id(),
                "owning-thread lease moved threads"
            );
            OWNER_THREAD.with(|owner| {
                assert_eq!(owner.get(), Some(self.id), "owning-thread lease replaced");
                owner.set(None);
            });
        }
        if let Some(retired) = self.retired.take() {
            retired.report(Ok(0));
        }
    }
}

/// The export a `wasixv2` thread starts at, as `wasixv2_thread_start(tid, start_arg)`.
pub const THREAD_ENTRY: &str = "wasixv2_thread_start";

/// What every thread of one process instantiates.
pub(crate) struct Image<S> {
    engine: Engine,
    module: Module,
    memory: SharedMemory,
    /// The low guard the process image protected in `memory` ([`crate::memory::LOW_GUARD`]).
    low_guard: u32,
    /// The main thread's stack in `memory`.
    main_stack: Range<u32>,
    layout: Option<wasixv2_syscalls::MemoryLayout>,
    buffers: Arc<Mutex<Vec<u32>>>,
    /// The ranges of `memory` that show shared objects read-only, for every thread.
    read_only: Arc<ReadOnly>,
    /// The frozen image `memory` is a private view of ([`crate::base`]).
    base: Option<Arc<Base>>,
    syscalls: Arc<S>,
    host: Arc<dyn Host>,
    /// The ABI of `module`.
    abi: Arc<dyn Abi<S>>,
    /// What the processes the threads start select their ABI from.
    abis: Arc<Abis<S>>,
    /// The process image's handoff ([`process::Handoff`]).
    handoff: Arc<process::Handoff>,
}

impl<S> Image<S> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        store: &Store,
        module: &Module,
        memory: SharedMemory,
        (low_guard, read_only, base): (u32, Arc<ReadOnly>, Option<Arc<Base>>),
        main_stack: Range<u32>,
        layout: Option<wasixv2_syscalls::MemoryLayout>,
        syscalls: Arc<S>,
        host: Arc<dyn Host>,
        abi: Arc<dyn Abi<S>>,
        abis: Arc<Abis<S>>,
        handoff: Arc<process::Handoff>,
    ) -> Arc<Self> {
        Arc::new(Self {
            engine: store.engine().clone(),
            module: module.clone(),
            memory,
            low_guard,
            main_stack,
            layout,
            buffers: Arc::default(),
            read_only,
            base,
            syscalls,
            host,
            abi,
            abis,
            handoff,
        })
    }
}

/// An execution-owned buffer on this memory, available again once its instance
/// has stopped. Like kernel/exit.c:exit_mm, retirement releases the task's storage;
/// keep the reservation for reuse rather than munmap through a task whose exec
/// may already have replaced its mm. Sharing the pool also covers CLONE_VM processes.
struct BufferLease {
    pool: Arc<Mutex<Vec<u32>>>,
    addr: Option<u32>,
}

impl Drop for BufferLease {
    fn drop(&mut self) {
        if let Some(addr) = self.addr {
            self.pool
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(addr);
        }
    }
}

/// Starts thread tasks of one process; `Image` without its syscalls type.
pub(crate) trait Start: Send + Sync {
    /// Starts the thread `task`, whose tid is `tid`, with `start_arg`.
    fn start(self: Arc<Self>, task: TaskId, tid: i32, start_arg: u32) -> io::Result<()>;

    /// The image of a new process on the same memory: its threads instantiate the same module,
    /// but its run is its own, with a handoff of its own ([`process::Handoff`]).
    fn for_process(&self) -> Arc<dyn Start>;
}

impl<S> Start for Image<S>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    fn start(self: Arc<Self>, task: TaskId, tid: i32, start_arg: u32) -> io::Result<()> {
        let host = self.host.clone();
        host.executor().spawn_thread(Box::new(move || {
            let profile = OwningThread::required(self.abis.requires_owning_thread());
            let cleanup = wasixv2_host::LoaderCleanup::new(self.host.clone());
            let syscalls = self.syscalls.clone();
            let profiled = matches!(&profile, Ok(Some(_)));
            let run = self.run(task, tid, start_arg, cleanup.clone(), profiled);
            Box::pin(async move {
                let profile = match profile {
                    Ok(profile) => profile,
                    Err(error) => {
                        let dropped =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(run)));
                        let error = match dropped {
                            Ok(()) => crate::reject_profile(&*syscalls, task, error),
                            Err(panic) => crate::host_panicked(&*syscalls, task, panic),
                        };
                        tracing::warn!(tid = task.0, %error, "cannot enter the owning-thread profile");
                        return;
                    }
                };
                match profile {
                    Some(profile) => {
                        if let Err(panic) = crate::catch_panic(profile.run(&cleanup, run)).await {
                            drop(crate::host_panicked(&*syscalls, task, panic));
                        }
                    }
                    None => cleanup.run(async move {
                        if let Err(panic) = crate::catch_panic(run).await {
                            drop(crate::host_panicked(&*syscalls, task, panic));
                        }
                    }).await,
                }
            })
        }))
    }

    fn for_process(&self) -> Arc<dyn Start> {
        Arc::new(Self {
            engine: self.engine.clone(),
            module: self.module.clone(),
            memory: self.memory.clone(),
            low_guard: self.low_guard,
            // The memory, so its stack region, is the creator's (Linux shares the mm).
            main_stack: self.main_stack.clone(),
            layout: self.layout.clone(),
            buffers: self.buffers.clone(),
            read_only: self.read_only.clone(),
            base: self.base.clone(),
            syscalls: self.syscalls.clone(),
            host: self.host.clone(),
            abi: self.abi.clone(),
            abis: self.abis.clone(),
            handoff: Arc::default(),
        })
    }
}

/// Starts the thread `child`, whose tid is `tid`, that the instance behind `env` created. On
/// failure the child is gone again and the caller reports `EAGAIN`, as `clone` does when the
/// host is out of threads.
pub(crate) async fn start<S: Control + 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    syscalls: &S,
    child: TaskId,
    tid: i32,
    start_arg: u32,
) -> bool {
    let image = env.read().await.data().threads.clone();
    // The first task of a new process on this memory leads a thread group of its own.
    let process = syscalls.tgid(child) == child;
    let started = match image {
        Some(image) if process => image.for_process().start(child, tid, start_arg),
        Some(image) => image.start(child, tid, start_arg),
        // The process has no shared memory to start threads on.
        None => Err(io::ErrorKind::Unsupported.into()),
    };
    if let Err(err) = &started {
        tracing::warn!(%err, "cannot start a guest thread");
        syscalls.abort_thread(child);
    }
    started.is_ok()
}

impl<S> Image<S>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    /// Runs the thread `task` to its end, on the host thread it was started on. A trap kills
    /// the thread group with its signal; then the kernel forgets the task.
    async fn run(
        self: Arc<Self>,
        task: TaskId,
        tid: i32,
        start_arg: u32,
        cleanup: Arc<wasixv2_host::LoaderCleanup>,
        profiled: bool,
    ) {
        let (syscalls, host) = (self.syscalls.clone(), self.host.clone());
        let (engine, abis) = (self.engine.clone(), self.abis.clone());
        match crate::catch_panic(self.call_entry(task, tid, start_arg, cleanup)).await {
            Ok(Ok(Ended::Exited(_))) => syscalls.release(task),
            Ok(Ok(Ended::Fault {
                task: at,
                signal,
                killed,
                report,
                ..
            })) => {
                // A thread's own task never goes on elsewhere (only a leader's does).
                let fault = (signal, killed);
                if process::fault_in(&*syscalls, &*host, task, at, fault, &report).is_some() {
                    syscalls.release(task);
                }
            }
            // Only a vfork parent's instance can end so, and a leader's run took over: the
            // leader's, which a thread's run is not.
            Ok(Ok(Ended::Detached(_))) => {
                tracing::warn!(tid = task.0, "a thread's instance ended without its task");
            }
            // Only a leader gets `execve`'s image (the kernel hands a thread's to it): the first
            // task of a process on its creator's memory, whose new image runs on its own
            // memory, in an instance of its own, as the process's run from now on.
            Ok(Ok(Ended::Exec(image))) => {
                process::spawner(engine, syscalls, host, abis).spawn(task, image);
            }
            Ok(Err(err)) => {
                tracing::warn!(tid = task.0, %err, "cannot start a guest thread's instance");
                process::fault(&*syscalls, task, Signal::SIGSEGV.0);
                syscalls.release(task);
            }
            // The actual factory's profile owns its exact cleanup drain. Report
            // only after that outer owner has observed physical settlement.
            Err(panic) if profiled => std::panic::resume_unwind(panic),
            Err(panic) => drop(crate::host_panicked(&*syscalls, task, panic)),
        }
    }

    async fn call_entry(
        self: Arc<Self>,
        task: TaskId,
        tid: i32,
        start_arg: u32,
        cleanup: Arc<wasixv2_host::LoaderCleanup>,
    ) -> Result<Ended, ExecError> {
        self.abi.attach_cleanup(&cleanup);
        let mut admission = if let Some(gate) = self.syscalls.loader_gate(task) {
            loop {
                match crate::Binding::new(&*self.abi, gate.clone(), task) {
                    Ok(admission) => break Some(admission),
                    Err(wasixv2_syscalls::Errno::EAGAIN) => self
                        .syscalls
                        .loader_sleep(task, &gate, wasixv2_syscalls::LoaderWait::Idle)
                        .await
                        .map_err(|e| ExecError::Load(crate::LoadError::Format(e)))?,
                    Err(error) => return Err(ExecError::Load(crate::LoadError::Format(error))),
                }
            }
        } else {
            None
        };
        let mut store = Store::new(self.engine.clone());
        self.abis.configure_store(&mut store)?;
        let invocations = crate::instantiation::NativeInvocations::bind(
            &mut store,
            self.abis.requires_owning_thread(),
        )?;
        let spawner = process::spawner(
            self.engine.clone(),
            self.syscalls.clone(),
            self.host.clone(),
            self.abis.clone(),
        );
        let env = FunctionEnv::new(
            &mut store,
            ExecEnv::new(
                self.syscalls.clone(),
                task,
                process::ProcExports::new(spawner, self.handoff.clone()),
                self.host.clone(),
            ),
        );
        env.as_mut(&mut store).cleanup = Some(cleanup);
        self.abis.install_hooks(&mut store, &env);
        let mut imports = crate::imports(&*self.abi, &mut store, &env, &self.module);
        let memory = self.memory.clone().try_attach(&mut store)?;
        if let Some(import) = self.module.imports().memories().next() {
            imports.define(import.module(), import.name(), memory.clone());
        }
        {
            let data = env.as_mut(&mut store);
            data.imports = imports.clone();
            data.memory = Some(memory.clone());
            data.low_guard = self.low_guard;
            data.main_stack = self.main_stack.clone();
            data.layout = self.layout.clone();
            data.read_only = self.read_only.clone();
            data.base = self.base.clone();
        }
        let (mut store, prepared, _, invocations) = crate::preparation::prepare(
            store,
            &env,
            &self.module,
            Role::Thread,
            &*self.abi,
            Some(&memory),
            &mut admission,
            invocations,
        )
        .await?;
        let hooks = env.as_ref(&store).instance_hooks().cloned();
        let configured =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<_, ExecError> {
                {
                    let data = env.as_mut(&mut store);
                    data.imports = wasmer::Imports::new();
                    if prepared.is_none() {
                        // Preserve the ordinary custom ABI's pre-bind environment. Native
                        // preparation alone needs the inherited real memory before bind.
                        data.memory = None;
                        data.low_guard = 0;
                        data.main_stack = 0..0;
                        data.layout = None;
                        data.read_only = Arc::default();
                        data.base = None;
                    }
                }
                if let Some(prepared) = &prepared {
                    imports.extend(prepared.imports());
                    crate::preparation::verify_memory(&self.module, &imports, Some(&memory))?;
                }
                if hooks.is_some() {
                    env.as_mut(&mut store).abi_imports = Some(imports.clone());
                }
                let attached = hooks
                    .as_ref()
                    .map(|hooks| {
                        hooks.prepare(
                            &self.module,
                            &mut wasmer::AsStoreMut::as_store_mut(&mut store),
                            &mut imports,
                        )
                    })
                    .transpose()
                    .map_err(ExecError::CustomBinding)?;
                let instance = Instance::new(&mut store, &self.module, &imports)?;
                let abi = match prepared {
                    Some(prepared) => prepared.bind(&instance, &mut store, task, Role::Thread)?,
                    None => self.abi.bind(&instance, &mut store, task, Role::Thread)?,
                };
                let asyncify = crate::asyncify::Asyncify::new(&instance, &mut store)?;
                let data = env.as_mut(&mut store);
                data.imports = imports.clone();
                data.memory = Some(memory);
                data.low_guard = self.low_guard;
                data.main_stack = self.main_stack.clone();
                data.layout = self.layout.clone();
                data.read_only = self.read_only.clone();
                data.base = self.base.clone();
                data.stack_pointer = crate::stack_pointer(&instance);
                data.abi = Some(abi.clone());
                data.threads = Some(self.clone());
                data.proc.set_module(&self.module);
                data.asyncify = asyncify;
                data.contexts_supported = !self.abis.requires_owning_thread();
                let memory = env.as_ref(&store).memory().unwrap().clone();
                if let Some(attached) = attached {
                    attached
                        .configure(
                            &self.module,
                            &mut wasmer::AsStoreMut::as_store_mut(&mut store),
                            &instance,
                            &memory,
                        )
                        .map_err(ExecError::CustomBinding)?;
                }
                Ok(abi)
            }));
        let abi = match configured {
            Ok(Ok(abi)) => abi,
            other => {
                invocations.close();
                if let Some(hooks) = &hooks {
                    hooks.deactivate(&mut wasmer::AsStoreMut::as_store_mut(&mut store));
                }
                match other {
                    Ok(Err(error)) => return Err(error),
                    Err(panic) => std::panic::resume_unwind(panic),
                    _ => unreachable!(),
                }
            }
        };
        let scope = env.as_ref(&store).loader_cleanup().unwrap().clone();
        let store = store.into_async();
        let mut attachments =
            crate::instantiation::AttachmentGuard::for_store(hooks, &store, scope, invocations);
        let entry = Entry::new(&env, &store);
        let mut buffer = BufferLease {
            pool: self.buffers.clone(),
            addr: self
                .buffers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop(),
        };
        if let Some(addr) = buffer.addr {
            entry
                .cx()
                .with(|env, _| {
                    if let Some(asyncify) = &mut env.asyncify {
                        asyncify.reserve_buffer(addr);
                    }
                })
                .await;
        }
        let prepared = abi.prepare_entry(&entry).await;
        let _registration = if prepared.is_ok() {
            match admission {
                Some(binding) => match binding.finish(&*self.abi, &*self.syscalls).await {
                    Ok(registration) => Some(registration),
                    Err(error) => {
                        if let Some(attachments) = &mut attachments {
                            attachments.settle().await;
                        }
                        return Err(ExecError::Load(crate::LoadError::Format(error)));
                    }
                },
                None => None,
            }
        } else {
            drop(admission);
            None
        };
        if let Err(error) = prepared {
            if let Some(attachments) = &mut attachments {
                attachments.settle().await;
            }
            return Err(ExecError::Load(crate::LoadError::other(error)));
        }
        let result = match crate::asyncify::prepare_buffer(entry.cx()).await {
            Ok(()) => {
                buffer.addr = entry
                    .cx()
                    .with(|env, _| env.asyncify.as_ref().and_then(|a| a.buffer_address()))
                    .await;
                crate::cooperative::drive(
                    &entry,
                    &*abi,
                    crate::contexts::drive(&entry, &mut attachments, {
                        let abi = abi.clone();
                        let env = entry.cx().env.clone();
                        let store = entry.store();
                        async move {
                            abi.start_thread(&Entry::new(&env, &store), tid, start_arg)
                                .await
                        }
                    }),
                )
                .await
            }
            Err(err) => Err(err),
        };
        let on_return = abi.on_thread_return();
        let result = match &mut attachments {
            Some(attachments) => attachments.finish(result).await,
            None => result,
        };
        let ended = crate::ended(entry.cx(), result, on_return, Role::Thread).await;
        if let Some(attachments) = &mut attachments {
            attachments.deactivate().await;
        }
        abi.retire(&entry)
            .await
            .map_err(|e| ExecError::Load(crate::LoadError::other(e)))?;
        Ok(ended)
    }
}

#[cfg(all(test, feature = "sys"))]
mod tests {
    use super::*;
    use crate::process::tests::{NoThreads, Recorder};
    use std::sync::atomic::Ordering;

    #[test]
    fn future_peer_prepares_import_overlay_before_instantiating_main() {
        use crate::preparation::tests::{Mode, Probe, ProbeAbi, control, layout, module};
        use std::sync::atomic::Ordering;
        let store = Store::default();
        let module = module(&store);
        let recorder = control(83);
        let probe = Arc::<Probe>::default();
        let selected: Arc<dyn Abi<Recorder>> = Arc::new(ProbeAbi {
            probe: probe.clone(),
            mode: Mode::Prepared,
        });
        let mut abis = Abis::new();
        abis.push(selected.clone());
        let abis = Arc::new(abis);
        let mut ready = futures::executor::block_on(crate::instantiate(
            store,
            &module,
            recorder.clone(),
            TaskId(1),
            Arc::new(NoThreads),
            abis.clone(),
            wasixv2_host::LoaderCleanup::new(Arc::new(NoThreads)),
            None,
        ))
        .unwrap();
        let shared = ready
            .env
            .as_ref(ready.store.as_ref().unwrap())
            .memory()
            .unwrap()
            .as_shared(ready.store.as_ref().unwrap())
            .unwrap();
        let image = Image::new(
            ready.store.as_ref().unwrap(),
            &module,
            shared,
            (0, Arc::default(), None),
            0..131072,
            Some(layout()),
            recorder.clone(),
            Arc::new(NoThreads),
            selected.clone(),
            abis,
            Arc::default(),
        );
        let store = ready.store.take().unwrap().into_async();
        futures::executor::block_on(async {
            let entry = Entry::new(&ready.env, &store);
            ready.abi.prepare_entry(&entry).await.unwrap();
            let registration = ready
                .admission
                .take()
                .unwrap()
                .finish(&*selected, &*recorder)
                .await
                .unwrap();
            let ended = image
                .call_entry(
                    TaskId(2),
                    2,
                    0,
                    wasixv2_host::LoaderCleanup::new(Arc::new(NoThreads)),
                )
                .await
                .unwrap();
            assert!(matches!(ended, Ended::Exited(0)));
            assert_eq!(
                *probe.roles.lock().unwrap(),
                vec![Role::Process, Role::Thread]
            );
            assert_eq!(probe.prepared_binds.load(Ordering::Relaxed), 2);
            assert_eq!(probe.ordinary_binds.load(Ordering::Relaxed), 0);
            assert_eq!(probe.guest_calls.load(Ordering::Relaxed), 1);
            assert_eq!(
                probe.claims.load(Ordering::Relaxed),
                3,
                "peer keeps its original admission"
            );
            assert!(recorder.loader.as_ref().unwrap().only_store(TaskId(1)));
            drop(registration);
        });
    }

    /// kernel/exit.c:exit_mm releases a retired task's execution resources.
    /// Repeated threads on one memory must not consume a fresh MiB each time.
    #[test]
    fn retired_thread_unwind_buffers_are_reused() {
        let store = Store::default();
        let module = Module::new(
            &store,
            wat::parse_str(
                r#"(module
            (import "env" "memory" (memory 4 64 shared))
            (export "memory" (memory 0))
            (global (export "__stack_pointer") (mut i32) (i32.const 131072))
            (func (export "asyncify_start_unwind") (param i32))
            (func (export "asyncify_stop_unwind"))
            (func (export "asyncify_start_rewind") (param i32))
            (func (export "asyncify_stop_rewind"))
            (func (export "wasixv2_thread_start") (param i32 i32) unreachable))"#,
            )
            .unwrap(),
        )
        .unwrap();
        let mut store = store;
        let memory =
            wasmer::Memory::new(&mut store, wasmer::MemoryType::new(4, Some(64), true)).unwrap();
        let control = Arc::new(Recorder::default());
        let layout = wasixv2_syscalls::MemoryLayout {
            image_end: 3 * 65536,
            data: 2 * 65536..3 * 65536,
            brk: 3 * 65536,
            maximum: 64 * 65536,
            reserved: Vec::new(),
        };
        let image = Image::new(
            &store,
            &module,
            memory.as_shared(&store).unwrap(),
            (0, Arc::default(), None),
            0..0,
            Some(layout),
            control.clone(),
            Arc::new(NoThreads),
            Arc::new(crate::wasixv2::Wasixv2),
            Arc::new(Abis::wasixv2()),
            Arc::default(),
        );
        for id in 2..6 {
            let ended = futures::executor::block_on(image.clone().call_entry(
                TaskId(id),
                id as i32,
                0,
                wasixv2_host::LoaderCleanup::new(Arc::new(NoThreads)),
            ))
            .unwrap();
            assert!(matches!(ended, Ended::Fault { .. }));
        }
        assert_eq!(control.buffer_allocations.load(Ordering::Relaxed), 1);
    }
}

#[cfg(all(test, feature = "sys"))]
mod admission_tests {
    use super::*;
    use crate::process::tests::{NoThreads, Recorder};
    #[test]
    fn wrong_image_owner_is_rejected_without_retrying_an_idle_gate() {
        let gate = wasixv2_syscalls::LoaderGate::new(91);
        let owner = Arc::new(());
        drop(gate.claim_image(&owner).unwrap());
        let mut control = Recorder::default();
        control.loader = Some(gate);
        let control = Arc::new(control);
        let mut store = Store::default();
        let module = Module::new(
            &store,
            wat::parse_str(r#"(module (import "env" "memory" (memory 1 2 shared)))"#).unwrap(),
        )
        .unwrap();
        let memory =
            wasmer::Memory::new(&mut store, wasmer::MemoryType::new(1, Some(2), true)).unwrap();
        let image = Image::new(
            &store,
            &module,
            memory.as_shared(&store).unwrap(),
            (0, Arc::default(), None),
            0..0,
            None,
            control,
            Arc::new(NoThreads),
            Arc::new(crate::Wasixv2),
            Arc::new(Abis::wasixv2()),
            Arc::default(),
        );
        let result = futures::executor::block_on(image.call_entry(
            TaskId(7),
            7,
            0,
            wasixv2_host::LoaderCleanup::new(Arc::new(NoThreads)),
        ));
        assert!(matches!(
            result,
            Err(ExecError::Load(crate::LoadError::Format(
                wasixv2_syscalls::Errno::EBUSY
            )))
        ));
    }
}
