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
        host.executor()
            .spawn_thread(Box::new(move || Box::pin(self.run(task, tid, start_arg))))
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
    async fn run(self: Arc<Self>, task: TaskId, tid: i32, start_arg: u32) {
        let (syscalls, host) = (self.syscalls.clone(), self.host.clone());
        let (engine, abis) = (self.engine.clone(), self.abis.clone());
        match crate::catch_panic(self.call_entry(task, tid, start_arg)).await {
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
            Err(panic) => drop(crate::host_panicked(&*syscalls, task, panic)),
        }
    }

    async fn call_entry(
        self: Arc<Self>,
        task: TaskId,
        tid: i32,
        start_arg: u32,
    ) -> Result<Ended, ExecError> {
        let mut store = Store::new(self.engine.clone());
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
        let mut imports = crate::imports(&*self.abi, &mut store, &env, &self.module);
        let memory = self.memory.clone().try_attach(&mut store)?;
        if let Some(import) = self.module.imports().memories().next() {
            imports.define(import.module(), import.name(), memory.clone());
        }
        let instance = Instance::new(&mut store, &self.module, &imports)?;
        let abi = self.abi.bind(&instance, &mut store, task, Role::Thread)?;
        let asyncify = crate::asyncify::Asyncify::new(&instance, &mut store)?;
        let data = env.as_mut(&mut store);
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

        let store = store.into_async();
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
        let result = match crate::asyncify::prepare_buffer(entry.cx()).await {
            Ok(()) => {
                buffer.addr = entry
                    .cx()
                    .with(|env, _| env.asyncify.as_ref().and_then(|a| a.buffer_address()))
                    .await;
                abi.start_thread(&entry, tid, start_arg).await
            }
            Err(err) => Err(err),
        };
        let on_return = abi.on_thread_return();
        Ok(crate::ended(entry.cx(), result, on_return, Role::Thread).await)
    }
}

#[cfg(all(test, feature = "sys"))]
mod tests {
    use super::*;
    use crate::process::tests::{NoThreads, Recorder};
    use std::sync::atomic::Ordering;

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
            let ended =
                futures::executor::block_on(image.clone().call_entry(TaskId(id), id as i32, 0))
                    .unwrap();
            assert!(matches!(ended, Ended::Fault { .. }));
        }
        assert_eq!(control.buffer_allocations.load(Ordering::Relaxed), 1);
    }
}
