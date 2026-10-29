//! Process images: `execve` replaces the instance with a new one of the image the kernel
//! loaded, vfork switches an instance between the parent and child tasks, and a fork child
//! starts from a copy of its parent's instance.
//!
//! A task switch is one field, [`ExecEnv::task`], which every import reads on entry.
//!
//! A process's run usually follows it from instance to instance until it exits. One `execve`
//! moves it to another host thread: a thread's, while the leader waits for its vfork child
//! ([`GuestAction::StartImage`]). The new image then runs on its own, and the instance the
//! leader's old image ran in ends with the child ([`Ended::Detached`]); its run returns the
//! status of the run that took over, which it gets through the image's [`Handoff`].

use std::{
    future::{Future, poll_fn},
    sync::{Arc, Mutex, PoisonError},
    task::{Poll, Waker},
};

use wasixv2_abi::Signal;
use wasixv2_host::Host;
use wasixv2_syscalls::{Control, GuestAction, Image, Resume, Syscalls, TaskId};
use wasmer::{AsyncFunctionEnvMut, Engine, Function, Module, RuntimeError, Store, Value};

use crate::{Ended, ExecEnv, ExecError, abi::Abis, asyncify};

/// Raised from an import to unwind the old image after `execve`, carrying the new one.
#[derive(Debug, thiserror::Error)]
#[error("execve replaced the image")]
pub(crate) struct ExecNew(pub(crate) Image);

/// Raised from an import to end an instance whose vfork child is done after the task the
/// instance's run belongs to went on in another instance ([`Resume::Gone`]).
///
/// [`Resume::Gone`]: wasixv2_syscalls::Resume::Gone
#[derive(Debug, thiserror::Error)]
#[error("the vfork parent's image went on in another instance")]
pub(crate) struct Detached;

/// Where the run that takes over a process leaves its result for the run it took over from,
/// shared by the instances of one process image: the process instance and its threads'. Only
/// one run takes over from an image, as the image is dead to every thread once it did.
#[derive(Default)]
pub(crate) struct Handoff(Mutex<HandoffState>);

#[derive(Default)]
struct HandoffState {
    result: Option<Result<i32, ExecError>>,
    /// The result was taken.
    taken: bool,
    waker: Option<Waker>,
}

impl Handoff {
    /// The run that took over ended with `result`; only the first result counts.
    fn complete(&self, result: Result<i32, ExecError>) {
        let waker = {
            let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            if state.taken || state.result.is_some() {
                return;
            }
            state.result = Some(result);
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// Waits for the run that took over to end, and returns how it ended.
    pub(crate) async fn wait(&self) -> Result<i32, ExecError> {
        poll_fn(|cx| {
            let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            match state.result.take() {
                Some(result) => {
                    state.taken = true;
                    Poll::Ready(result)
                }
                None => {
                    state.waker = Some(cx.waker().clone());
                    Poll::Pending
                }
            }
        })
        .await
    }
}

/// The run that takes over a process, once it runs, with the [`Handoff`] it reports to. A run
/// the host drops before it ends reports [`ExecError::Dropped`]. (One the host never starts
/// reports what killing the process gave; see [`Spawner::take_over`].)
struct Takeover(Arc<Handoff>);

impl Takeover {
    fn report(self, result: Result<i32, ExecError>) {
        self.0.complete(result);
    }
}

impl Drop for Takeover {
    fn drop(&mut self) {
        self.0.complete(Err(ExecError::Dropped));
    }
}

/// Starts the processes an instance creates on instances of their own: the image of a vfork
/// child that called `execve`, a fork child, and the new image of a leader that waits for its
/// vfork child; the type-erased part of the execution layer that the return-to-guest path can
/// reach.
pub(crate) trait Spawn: Send + Sync {
    fn spawn(&self, task: TaskId, image: Image);
    fn fork(&self, task: TaskId, image: asyncify::ForkImage);
    /// Starts `image` as `task`, whose run goes on there, and reports how it ends to
    /// `handoff`.
    fn take_over(&self, task: TaskId, image: Image, handoff: Arc<Handoff>);
}

pub(crate) struct Spawner<S> {
    engine: Engine,
    syscalls: Arc<S>,
    host: Arc<dyn Host>,
    /// The ABIs the images of the processes it starts select from.
    abis: Arc<Abis<S>>,
}

pub(crate) fn spawner<S>(
    engine: Engine,
    syscalls: Arc<S>,
    host: Arc<dyn Host>,
    abis: Arc<Abis<S>>,
) -> Arc<dyn Spawn>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    Arc::new(Spawner {
        engine,
        syscalls,
        host,
        abis,
    })
}

impl<S> Spawn for Spawner<S>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    fn spawn(&self, task: TaskId, image: Image) {
        let (engine, syscalls, host, abis) = (
            self.engine.clone(),
            self.syscalls.clone(),
            self.host.clone(),
            self.abis.clone(),
        );
        // Without a thread the process died; nothing waits for its status.
        let _ = self.start(task, move || {
            let run = run_new_image(engine, image, syscalls, task, host, abis);
            async move { report_failure(task, run.await) }
        });
    }

    fn fork(&self, task: TaskId, image: asyncify::ForkImage) {
        let (engine, syscalls, host, abis) = (
            self.engine.clone(),
            self.syscalls.clone(),
            self.host.clone(),
            self.abis.clone(),
        );
        // Without a thread the process died; nothing waits for its status.
        let _ = self.start(task, move || {
            let run = run_forked(engine, image, syscalls.clone(), task, host, abis);
            async move {
                let result = crate::catch_panic(run)
                    .await
                    .unwrap_or_else(|panic| Err(crate::host_panicked(&*syscalls, task, panic)));
                report_failure(task, result);
            }
        });
    }

    fn take_over(&self, task: TaskId, image: Image, handoff: Arc<Handoff>) {
        let (engine, syscalls, host, abis) = (
            self.engine.clone(),
            self.syscalls.clone(),
            self.host.clone(),
            self.abis.clone(),
        );
        let done = handoff.clone();
        let started = self.start(task, move || {
            let takeover = Takeover(done);
            let run = run_new_image(engine, image, syscalls, task, host, abis);
            async move { takeover.report(run.await) }
        });
        // The process died of SIGSEGV, as one whose image cannot start does.
        if let Err(status) = started {
            handoff.complete(Ok(status));
        }
    }
}

/// Logs a process image that failed; its run ended the process.
fn report_failure(task: TaskId, result: Result<i32, ExecError>) {
    if let Err(err) = result {
        tracing::warn!(tid = task.0, %err, "process image failed");
    }
}

impl<S> Spawner<S>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    /// Runs the process `task` on a host thread of its own. The run ends the process itself when
    /// its image fails; when no thread can start, the process dies of SIGSEGV here, and
    /// `Err` has its status.
    fn start<F, R>(&self, task: TaskId, run: R) -> Result<(), i32>
    where
        R: FnOnce() -> F + Send + 'static,
        F: Future<Output = ()> + 'static,
    {
        let start = Box::new(move || {
            Box::pin(wasixv2_syscalls::as_task(task, run()))
                as wasixv2_host::LocalFuture<'static, ()>
        });
        if let Err(err) = self.host.executor().spawn_thread(start) {
            tracing::warn!(%err, "cannot start a process on a thread of its own");
            let status = fault(&*self.syscalls, task, Signal::SIGSEGV.0);
            self.syscalls.release(task);
            return Err(status);
        }
        Ok(())
    }
}

/// Runs `module` as `task`, then every image its `execve` calls load, until the process exits;
/// releases the task then.
pub(crate) async fn run<S>(
    store: Store,
    module: &Module,
    syscalls: Arc<S>,
    task: TaskId,
    host: Arc<dyn Host>,
    abis: Arc<Abis<S>>,
) -> Result<i32, ExecError>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    let engine = store.engine().clone();
    let first = crate::run_image(
        store,
        module,
        syscalls.clone(),
        task,
        host.clone(),
        abis.clone(),
    )
    .await;
    finish(engine, first, syscalls, task, host, abis).await
}

/// Runs a fork child from the copy of its parent's instance, then every image its `execve`
/// calls load, until the process exits; releases the task then.
async fn run_forked<S>(
    engine: Engine,
    image: asyncify::ForkImage,
    syscalls: Arc<S>,
    task: TaskId,
    host: Arc<dyn Host>,
    abis: Arc<Abis<S>>,
) -> Result<i32, ExecError>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    let store = Store::new(engine.clone());
    let first = crate::run_forked(
        store,
        image,
        syscalls.clone(),
        task,
        host.clone(),
        abis.clone(),
    )
    .await;
    finish(engine, first, syscalls, task, host, abis).await
}

/// Runs `image`, which `kernel_execve` made `task`'s, then every image its `execve` calls load,
/// until the process exits; releases the task then. `engine` is the engine the process would
/// have run on, which the loader may compile with.
///
/// Only the images the process's own `execve` calls load die of SIGSEGV when they do not start:
/// `image` is the embedder's program, so it fails as [`run`]'s module does, with the error, the
/// process killed.
pub(crate) async fn run_exec<S>(
    mut engine: Engine,
    image: Image,
    syscalls: Arc<S>,
    task: TaskId,
    host: Arc<dyn Host>,
    abis: Arc<Abis<S>>,
) -> Result<i32, ExecError>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    let first = match syscalls.exec_wait(task).await {
        // Killed meanwhile: it ends as the kill says.
        Err(status) => Ok(Ended::Exited(status)),
        Ok(()) => {
            let loaded = abis.loader().load(&engine, &image).await;
            drop(image);
            match loaded {
                Ok((image_engine, module)) => {
                    engine = image_engine;
                    let store = Store::new(engine.clone());
                    let run = crate::run_image(
                        store,
                        &module,
                        syscalls.clone(),
                        task,
                        host.clone(),
                        abis.clone(),
                    );
                    run.await
                }
                Err(err) => Err(ExecError::Load(err)),
            }
        }
    };
    finish(engine, first, syscalls, task, host, abis).await
}

/// Follows `task`'s process from the end of its first image's run through the images its
/// `execve` calls load, until it exits; releases the task then. An image that cannot start kills
/// the process with SIGSEGV.
async fn finish<S>(
    mut engine: Engine,
    first: Result<Ended, ExecError>,
    syscalls: Arc<S>,
    task: TaskId,
    host: Arc<dyn Host>,
    abis: Arc<Abis<S>>,
) -> Result<i32, ExecError>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    let mut ended = match first {
        Ok(ended) => ended,
        Err(err) => {
            fault(&*syscalls, task, Signal::SIGSEGV.0);
            syscalls.release(task);
            return Err(err);
        }
    };
    let status = loop {
        let image = match ended {
            // The task runs on in the instance that took over, which releases it.
            Ended::Detached(handoff) => return handoff.wait().await,
            Ended::Exited(status) => break status,
            Ended::Fault {
                task: at,
                signal,
                report,
                handoff,
            } => match fault_in(&*syscalls, &*host, task, at, signal, &report) {
                Some(status) => break status,
                // The trap was the vfork child's whose parent went on elsewhere.
                None => {
                    let handoff = handoff.expect("only an instance's trap leaves a vfork parent");
                    return handoff.wait().await;
                }
            },
            Ended::Exec(image) => image,
        };
        // `de_thread`: the old image is gone from every thread before the new one starts.
        if let Err(status) = syscalls.exec_wait(task).await {
            break status;
        }
        let loaded = abis.loader().load(&engine, &image).await;
        drop(image);
        ended = match loaded {
            Ok((image_engine, module)) => {
                engine = image_engine;
                let store = Store::new(engine.clone());
                let run = crate::run_image(
                    store,
                    &module,
                    syscalls.clone(),
                    task,
                    host.clone(),
                    abis.clone(),
                );
                run.await.unwrap_or_else(|err| Ended::Fault {
                    task,
                    handoff: None,
                    signal: Signal::SIGSEGV.0,
                    report: format!(
                        "wasixv2: cannot start the image process {} loaded with execve, \
                         killing it with SIGSEGV: {err}",
                        task.0
                    ),
                })
            }
            // Past the point of no return, a binary that does not load is a SIGSEGV.
            Err(err) => Ended::Fault {
                task,
                handoff: None,
                signal: Signal::SIGSEGV.0,
                report: format!(
                    "wasixv2: process {} loaded a module with execve that does not compile, \
                     killing it with SIGSEGV: {err}",
                    task.0
                ),
            },
        };
    };
    // A zombie leader stays until its group is empty.
    syscalls.group_wait(task).await;
    syscalls.release(task);
    Ok(status)
}

/// The image of a vfork child's `execve`.
async fn run_new_image<S>(
    engine: Engine,
    image: Image,
    syscalls: Arc<S>,
    task: TaskId,
    host: Arc<dyn Host>,
    abis: Arc<Abis<S>>,
) -> Result<i32, ExecError>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    // `de_thread`: the old image is gone from every thread before the new one starts.
    if let Err(status) = syscalls.exec_wait(task).await {
        syscalls.group_wait(task).await;
        syscalls.release(task);
        return Ok(status);
    }
    let loaded = abis.loader().load(&engine, &image).await;
    drop(image);
    let (engine, module) = match loaded {
        Ok(loaded) => loaded,
        Err(err) => {
            tracing::warn!(tid = task.0, %err, "execve loaded a module that does not compile");
            let status = fault(&*syscalls, task, Signal::SIGSEGV.0);
            syscalls.release(task);
            return Ok(status);
        }
    };
    crate::run_with_abis(Store::new(engine), &module, syscalls, task, host, abis).await
}

/// Kills `task`'s thread group with the fault `signal` and returns its exit status.
pub(crate) fn fault<S: Control>(syscalls: &S, task: TaskId, signal: i32) -> i32 {
    syscalls.fault(task, signal);
    exit_status(syscalls, task)
}

/// A trap of the instance of `task` while it ran as `at`: `task` itself, or a vfork child
/// running in its instance. The child dies of the fault; the parent's stack went with the
/// instance, so it cannot resume either and dies of SIGSEGV, unless the parent went on in
/// another instance ([`Resume::Gone`]), where it is not touched: then `None`, and the run
/// ends with the one that took over. `report` goes to `host` when the fault kills a process:
/// a trap while its group is exiting anyway is part of tearing it down. Returns `task`'s exit
/// status.
pub(crate) fn fault_in<S: Control>(
    syscalls: &S,
    host: &dyn Host,
    task: TaskId,
    at: TaskId,
    signal: i32,
    report: &str,
) -> Option<i32> {
    if syscalls.fault(at, signal) {
        host.report(report);
    }
    if at == task {
        return Some(exit_status(syscalls, task));
    }
    // The child's end, which its exit queued, says what becomes of the parent.
    let mut gone = false;
    while let Some(action) = syscalls.take_pending(at) {
        if let GuestAction::VforkReturn { parent, .. } = action {
            gone |= parent.resumes == Resume::Gone;
        }
    }
    syscalls.release(at);
    (!gone).then(|| fault(syscalls, task, Signal::SIGSEGV.0))
}

/// The status of the exit pending on `task`, taking everything else that is.
pub(crate) fn exit_status<S: Control>(syscalls: &S, task: TaskId) -> i32 {
    let mut status = 128 + Signal::SIGKILL.0;
    while let Some(action) = syscalls.take_pending(task) {
        if let GuestAction::Exit(exit) = action {
            status = exit;
        }
    }
    status
}

/// Makes the instance behind `env` run as `task` from now on.
pub(crate) async fn switch<S: Control + 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    task: TaskId,
) {
    env.write().await.data_mut().switch(task);
}

/// Starts the image `task` loaded with `execve` while it was a vfork child.
pub(crate) async fn spawn_image<S: 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    task: TaskId,
    image: Image,
) {
    let spawner = env.read().await.data().proc.spawner.clone();
    spawner.spawn(task, image);
}

/// Starts `image`, which an `execve` of the task the instance behind `env` runs as gave the
/// leader `task` while the leader waited for its vfork child, as `task` on an instance of its
/// own. The run of the leader's old image goes on there.
pub(crate) async fn take_over<S: 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    task: TaskId,
    image: Image,
) {
    let (spawner, handoff) = {
        let env = env.read().await;
        let proc = &env.data().proc;
        (proc.spawner.clone(), proc.handoff.clone())
    };
    spawner.take_over(task, image, handoff);
}

/// What the execution layer needs of an instance for processes.
pub(crate) struct ProcExports {
    pub(crate) spawner: Arc<dyn Spawn>,
    /// The instance's module, which a fork child instantiates.
    pub(crate) module: Option<Module>,
    /// The handoff of the instance's process image ([`Handoff`]).
    pub(crate) handoff: Arc<Handoff>,
}

impl ProcExports {
    pub(crate) fn new(spawner: Arc<dyn Spawn>, handoff: Arc<Handoff>) -> Self {
        Self {
            spawner,
            module: None,
            handoff,
        }
    }

    pub(crate) fn set_module(&mut self, module: &Module) {
        self.module = Some(module.clone());
    }
}

/// The export a vfork parent resumes through, as `wasixv2_vfork_return(child_tid)`.
pub const VFORK_RETURN: &str = "wasixv2_vfork_return";

/// Calls the guest's vfork return, `function`, with the child's pid as the parent sees it, in
/// the vfork parent, which unwinds to the vfork point with an exception; returns that error
/// for the import to raise.
pub(crate) async fn vfork_return<S: 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    function: Option<&Function>,
    pid: i32,
) -> RuntimeError {
    let Some(function) = function else {
        return RuntimeError::new(format!(
            "a vfork child ended, but the guest has no `{VFORK_RETURN}`"
        ));
    };
    let args = [Value::I32(pid)];
    match asyncify::call(env, None, function, &args).await {
        Err(err) => err,
        Ok(_) => RuntimeError::new(format!("`{VFORK_RETURN}` returned")),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, VecDeque},
        ops::Range,
        time::Duration,
    };

    use wasixv2_host::{Executor, LocalFuture};
    use wasixv2_syscalls::{
        AtomicWait, AtomicWord, Caller, CpuTime, ExitMm, GroupStop, PidView, SharedMapping,
        SharedMappings, SysResult, VforkParent, VforkWait,
    };

    use super::*;

    /// Control that records faults and releases; a fault leaves the exit it causes pending.
    #[derive(Default)]
    struct Recorder {
        pending: Mutex<HashMap<TaskId, VecDeque<GuestAction>>>,
        faulted: Mutex<Vec<(TaskId, i32)>>,
        released: Mutex<Vec<TaskId>>,
        /// The tasks that left their memory and the ones that faulted, in order.
        ended: Mutex<Vec<(&'static str, TaskId)>>,
    }

    impl Recorder {
        fn queue(&self, task: TaskId, action: GuestAction) {
            let mut pending = self.pending.lock().unwrap();
            pending.entry(task).or_default().push_back(action);
        }
    }

    impl Syscalls for Recorder {}

    impl Control for Recorder {
        fn take_pending(&self, task: TaskId) -> Option<GuestAction> {
            self.pending.lock().unwrap().get_mut(&task)?.pop_front()
        }

        fn take_image(&self, _: TaskId) -> Option<Image> {
            None
        }

        fn fault(&self, task: TaskId, signal: i32) -> bool {
            self.ended.lock().unwrap().push(("fault", task));
            self.faulted.lock().unwrap().push((task, signal));
            self.queue(task, GuestAction::Exit(128 + signal));
            true
        }

        fn release(&self, task: TaskId) {
            self.released.lock().unwrap().push(task);
        }
    }

    impl CpuTime for Recorder {}

    impl PidView for Recorder {}

    impl ExitMm for Recorder {
        fn exit_mm<'a>(&'a self, caller: &'a Caller<'a>) -> LocalFuture<'a, ()> {
            self.ended.lock().unwrap().push(("exit_mm", caller.task()));
            Box::pin(async {})
        }
    }

    impl GroupStop for Recorder {
        fn stop_wait(&self, _: TaskId) -> LocalFuture<'_, ()> {
            Box::pin(async {})
        }
    }

    impl VforkWait for Recorder {
        fn vfork_wait(&self, _: TaskId) -> LocalFuture<'_, ()> {
            Box::pin(async {})
        }
    }

    impl AtomicWait for Recorder {
        fn atomic_wait<'a>(
            &'a self,
            _: &'a Caller<'a>,
            _: u32,
            _: AtomicWord,
            _: Option<Duration>,
        ) -> LocalFuture<'a, SysResult<i32>> {
            Box::pin(async { Ok(0) })
        }

        fn atomic_notify(&self, _: TaskId, _: u32, _: u32) -> u32 {
            0
        }
    }

    impl SharedMappings for Recorder {
        fn shared_mappings(&self, _: TaskId) -> Vec<SharedMapping> {
            Vec::new()
        }

        fn wipe_on_fork(&self, _: TaskId) -> Vec<Range<u64>> {
            Vec::new()
        }
    }

    /// A host that cannot start threads.
    struct NoThreads;

    impl Host for NoThreads {
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

        fn report(&self, _: &str) {}
    }

    impl Executor for NoThreads {
        fn spawn_local(&self, _: LocalFuture<'static, ()>) {
            unreachable!("nothing runs in the background")
        }

        fn spawn_blocking(&self, work: Box<dyn FnOnce() + Send>) -> LocalFuture<'static, ()> {
            Box::pin(async move { work() })
        }
    }

    const PARENT: TaskId = TaskId(2);
    const CHILD: TaskId = TaskId(3);

    /// A trap of a vfork child whose parent's image went on in another instance ends the child
    /// alone: the parent is neither killed nor released here, as the run that took it over
    /// owns it.
    /// A trap kills the task as any fatal signal does on Linux, whose `do_exit` runs `exit_mm`
    /// (`mm_release`: the robust futex list, `clear_child_tid` and its futex wake) for every
    /// exit: the instance hands the kernel the task's memory, still there, before the fault
    /// ends the group.
    #[cfg(feature = "sys")]
    #[test]
    fn a_trapped_task_leaves_its_memory_before_its_fault_kills_the_group() {
        let wasm = wat::parse_str(
            r#"(module
                (import "wasixv2" "sched_yield" (func (result i32)))
                (memory (export "memory") 1)
                (func (export "_start") unreachable))"#,
        )
        .unwrap();
        let store = Store::default();
        let module = wasmer::Module::new(&store, wasm).unwrap();
        let control = Arc::new(Recorder::default());
        let run = crate::run(store, &module, control.clone(), PARENT, Arc::new(NoThreads));
        let status = futures::executor::block_on(run).unwrap();
        assert_eq!(status, 128 + Signal::SIGILL.0);
        let ended = control.ended.lock().unwrap();
        assert_eq!(ended[..2], [("exit_mm", PARENT), ("fault", PARENT)]);
    }

    #[test]
    fn a_trap_of_a_detached_vfork_child_leaves_its_parent_alone() {
        let control = Recorder::default();
        let parent = VforkParent {
            task: PARENT,
            resumes: Resume::Gone,
            pid: CHILD.0 as i32,
        };
        control.queue(
            CHILD,
            GuestAction::VforkReturn {
                parent,
                child: CHILD,
            },
        );
        let sigill = Signal::SIGILL.0;
        let ended = fault_in(&control, &NoThreads, PARENT, CHILD, sigill, "trap");
        assert_eq!(ended, None);
        assert_eq!(*control.faulted.lock().unwrap(), [(CHILD, sigill)]);
        assert_eq!(*control.released.lock().unwrap(), [CHILD]);

        // A parent that waits dies with its stack.
        let control = Recorder::default();
        let parent = VforkParent {
            resumes: Resume::Returns,
            ..parent
        };
        control.queue(
            CHILD,
            GuestAction::VforkReturn {
                parent,
                child: CHILD,
            },
        );
        let ended = fault_in(&control, &NoThreads, PARENT, CHILD, sigill, "trap");
        assert_eq!(ended, Some(128 + Signal::SIGSEGV.0));
    }

    /// A take-over that gets no thread kills the process as an image that cannot start does,
    /// releases it, and reports that status to the run it took over from.
    #[test]
    fn a_take_over_without_a_thread_reports_the_kill() {
        let control = Arc::new(Recorder::default());
        let abis = Arc::new(Abis::wasixv2());
        let spawner = spawner(
            Engine::default(),
            control.clone(),
            Arc::new(NoThreads),
            abis,
        );
        let handoff = Arc::new(Handoff::default());
        spawner.take_over(PARENT, Image::copied(Vec::new()), handoff.clone());
        let status = futures::executor::block_on(handoff.wait());
        assert_eq!(status.ok(), Some(128 + Signal::SIGSEGV.0));
        assert_eq!(*control.released.lock().unwrap(), [PARENT]);
    }
}
