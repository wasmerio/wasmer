#[cfg(feature = "journal")]
use crate::{journal::JournalEffector, unwind, WasiResult};
use crate::{
    journal::SnapshotTrigger, runtime::module_cache::ModuleHash, WasiEnv, WasiRuntimeError,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    convert::TryInto,
    sync::{
        atomic::{AtomicI32, AtomicU32, Ordering},
        Arc, Condvar, Mutex, MutexGuard, RwLock, Weak,
    },
    time::Duration,
};
use tracing::trace;
use wasmer::{FromToNativeWasmType, FunctionEnvMut};
use wasmer_wasix_types::{
    types::Signal,
    wasi::{Errno, ExitCode, Snapshot0Clockid},
};

use crate::{
    os::task::signal::WasiSignalInterval, syscalls::platform_clock_time_get, WasiThread,
    WasiThreadHandle, WasiThreadId,
};

use super::{
    control_plane::{ControlPlaneError, WasiControlPlaneHandle},
    signal::{SignalDeliveryError, SignalHandlerAbi},
    task_join_handle::OwnedTaskStatus,
    thread::WasiMemoryLayout,
};

/// Represents the ID of a sub-process
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct WasiProcessId(u32);

impl WasiProcessId {
    pub fn raw(&self) -> u32 {
        self.0
    }
}

impl From<i32> for WasiProcessId {
    fn from(id: i32) -> Self {
        Self(id as u32)
    }
}

impl From<WasiProcessId> for i32 {
    fn from(val: WasiProcessId) -> Self {
        val.0 as i32
    }
}

impl From<u32> for WasiProcessId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}

impl From<WasiProcessId> for u32 {
    fn from(val: WasiProcessId) -> Self {
        val.0
    }
}

impl std::fmt::Display for WasiProcessId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Debug for WasiProcessId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Represents a process running within the compute state
/// TODO: fields should be private and only accessed via methods.
#[derive(Debug, Clone)]
pub struct WasiProcess {
    state: Arc<State>,
}

#[derive(Debug)]
struct State {
    /// Unique ID of this process
    pid: WasiProcessId,
    /// Hash of the module that this process is using
    module_hash: ModuleHash,
    /// List of all the children spawned from this thread
    parent: Option<Weak<RwLock<WasiProcessInner>>>,
    /// Reference back to the compute engine
    // TODO: remove this reference, access should happen via separate state instead
    // (we don't want cyclical references)
    compute: WasiControlPlaneHandle,

    /// This process was instructed to terminate and should stop executing
    /// immediately.
    /// The value is either 0 (= shouldl not terminate), or an exit code.
    should_terminate_with_code: AtomicI32,

    /// Reference to the exit code for the main thread
    status: Arc<OwnedTaskStatus>,

    /// Number of threads waiting for children to exit
    waiting: Arc<AtomicU32>,

    /// The inner protected region of the process with a conditional
    /// variable that is used for coordination such as checksums.
    lock_condvar: Condvar,
    inner: Mutex<WasiProcessInner>,
}

/// Represents a freeze of all threads to perform some action
/// on the total state-machine. This is normally done for
/// things like snapshots which require the memory to remain
/// stable while it performs a diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProcessCheckpoint {
    /// No checkpoint will take place and the process
    /// should just execute as per normal
    Execute,
    /// The process needs to take a snapshot of the
    /// memory and state-machine
    Snapshot { trigger: SnapshotTrigger },
}

// TODO(theduke): this struct should be private!
#[derive(Debug)]
pub struct WasiProcessInner {
    /// Unique ID of this process
    pub pid: WasiProcessId,
    /// The threads that make up this process
    pub threads: HashMap<WasiThreadId, WasiThread>,
    /// Number of threads running for this process
    pub thread_count: u32,
    /// Signals that will be triggered at specific intervals
    pub signal_intervals: HashMap<Signal, WasiSignalInterval>,
    /// Child processes.
    pub children: Vec<WasiProcess>,
    /// Represents a checkpoint which blocks all the threads
    /// and then executes some maintenance action
    pub checkpoint: ProcessCheckpoint,
}

pub enum MaybeCheckpointResult<'a> {
    NotThisTime(FunctionEnvMut<'a, WasiEnv>),
    Unwinding,
}

impl WasiProcess {
    pub fn new(pid: WasiProcessId, module_hash: ModuleHash, plane: WasiControlPlaneHandle) -> Self {
        WasiProcess {
            state: Arc::new(State {
                pid,
                module_hash,
                parent: None,
                compute: plane,
                lock_condvar: Condvar::new(),
                inner: Mutex::new(WasiProcessInner {
                    pid,
                    threads: Default::default(),
                    thread_count: Default::default(),
                    signal_intervals: Default::default(),
                    children: Default::default(),
                    checkpoint: ProcessCheckpoint::Execute,
                }),
                status: Arc::new(OwnedTaskStatus::default()),
                should_terminate_with_code: AtomicI32::new(0),
                waiting: Arc::new(AtomicU32::new(0)),
            }),
        }
    }

    pub fn handle(&self) -> WasiProcessHandle {
        WasiProcessHandle::new(&self.state)
    }

    #[inline]
    pub fn module_hash(&self) -> &ModuleHash {
        &self.state.module_hash
    }

    #[inline]
    pub fn status(&self) -> &Arc<OwnedTaskStatus> {
        &self.state.status
    }

    #[inline]
    pub fn control_plane(&self) -> &WasiControlPlaneHandle {
        &self.state.compute
    }

    /// Gets the process ID of this process
    #[inline]
    pub fn pid(&self) -> WasiProcessId {
        self.state.pid
    }

    /// Notify the shared lock condvar to trigger waiters.
    pub fn lock_notify_all(&self) {
        self.state.lock_condvar.notify_all();
    }

    pub fn lock_wait<'a>(
        &self,
        guard: MutexGuard<'a, WasiProcessInner>,
    ) -> MutexGuard<'a, WasiProcessInner> {
        self.state.lock_condvar.wait(guard).unwrap()
    }

    /// Whether the process should terminate.
    ///
    /// Returns `None` if the process should not terminate, or `Some(code)` if
    /// the process should terminate with the given exit code.
    pub fn should_terminate_with_code(&self) -> Option<ExitCode> {
        let v = self
            .state
            .should_terminate_with_code
            .load(Ordering::Acquire);
        if v == 0 {
            None
        } else {
            Some(ExitCode::from_native(v))
        }
    }

    /// Gets the process ID of the parent process
    pub fn ppid(&self) -> WasiProcessId {
        self.state
            .parent
            .iter()
            .filter_map(|parent| parent.upgrade())
            .map(|parent| parent.read().unwrap().pid)
            .next()
            .unwrap_or(WasiProcessId(0))
    }

    /// Gains access to the process internals
    // TODO: Make this private, all inner access should be exposed with methods.
    pub fn lock(&self) -> MutexGuard<'_, WasiProcessInner> {
        self.state.inner.lock().unwrap()
    }

    /// Creates a a thread and returns it
    pub fn new_thread(
        &self,
        layout: WasiMemoryLayout,
    ) -> Result<WasiThreadHandle, ControlPlaneError> {
        let control_plane = self.state.compute.must_upgrade();
        let task_count_guard = control_plane.register_task()?;

        // Determine if its the main thread or not
        let is_main = {
            let inner = self.lock();
            inner.thread_count == 0
        };

        // Generate a new process ID (this is because the process ID and thread ID
        // address space must not overlap in libc). For the main proecess the TID=PID
        let tid: WasiThreadId = if is_main {
            self.pid().raw().into()
        } else {
            let tid: u32 = control_plane.generate_id()?.into();
            tid.into()
        };

        // The wait finished should be the process version if its the main thread
        let mut inner = self.lock();
        let finished = if is_main {
            self.state.status.clone()
        } else {
            Arc::new(OwnedTaskStatus::default())
        };

        // Insert the thread into the pool
        let ctrl = WasiThread::new(self.pid(), tid, is_main, finished, task_count_guard, layout);
        inner.threads.insert(tid, ctrl.clone());
        inner.thread_count += 1;

        Ok(WasiThreadHandle::new(ctrl, self.handle()))
    }

    /// Gets a reference to a particular thread
    pub fn get_thread(&self, tid: &WasiThreadId) -> Option<WasiThread> {
        self.lock().threads.get(tid).cloned()
    }

    /// Signals a particular thread in the process
    pub fn signal_thread(&self, tid: &WasiThreadId, signal: Signal) {
        // Sometimes we will signal the process rather than the thread hence this libc hardcoded value
        let mut tid = tid.raw();
        if tid == 1073741823 {
            tid = self.pid().raw();
        }
        let tid: WasiThreadId = tid.into();

        let pid = self.pid();
        tracing::trace!(%pid, %tid, "signal-thread({:?})", signal);

        let inner = self.lock();
        if let Some(thread) = inner.threads.get(&tid) {
            thread.signal(signal);
        } else {
            trace!(
                "wasi[{}]::lost-signal(tid={}, sig={:?})",
                self.pid(),
                tid,
                signal
            );
        }
    }

    /// Signals all the threads in this process
    pub fn signal_process(&self, signal: Signal) {
        let pid = self.pid();
        tracing::trace!(%pid, "signal-process({:?})", signal);

        {
            let inner = self.lock();
            if self.state.waiting.load(Ordering::Acquire) > 0 {
                let mut triggered = false;
                for child in inner.children.iter() {
                    child.signal_process(signal);
                    triggered = true;
                }
                if triggered {
                    return;
                }
            }
        }
        let inner = self.lock();
        for thread in inner.threads.values() {
            thread.signal(signal);
        }
    }

    /// Signals one of the threads every interval
    pub fn signal_interval(&self, signal: Signal, interval: Option<Duration>, repeat: bool) {
        let mut inner = self.lock();

        let interval = match interval {
            None => {
                inner.signal_intervals.remove(&signal);
                return;
            }
            Some(a) => a,
        };

        let now = platform_clock_time_get(Snapshot0Clockid::Monotonic, 1_000_000).unwrap() as u128;
        inner.signal_intervals.insert(
            signal,
            WasiSignalInterval {
                signal,
                interval,
                last_signal: now,
                repeat,
            },
        );
    }

    /// Returns the number of active threads for this process
    pub fn active_threads(&self) -> u32 {
        let inner = self.lock();
        inner.thread_count
    }

    /// Waits until the process is finished.
    pub async fn join(&self) -> Result<ExitCode, Arc<WasiRuntimeError>> {
        let _guard = WasiProcessWait::new(self);
        self.state.status.await_termination().await
    }

    /// Attempts to join on the process
    pub fn try_join(&self) -> Option<Result<ExitCode, Arc<WasiRuntimeError>>> {
        self.state.status.status().into_finished()
    }

    /// Waits for all the children to be finished
    pub async fn join_children(&self) -> Option<Result<ExitCode, Arc<WasiRuntimeError>>> {
        let _guard = WasiProcessWait::new(self);
        let children: Vec<_> = {
            let inner = self.lock();
            inner.children.clone()
        };
        if children.is_empty() {
            return None;
        }
        let mut waits = Vec::new();
        for child in children {
            if let Some(process) = self.state.compute.must_upgrade().get_process(child.pid()) {
                let self_ = self.clone();
                waits.push(async move {
                    let join = process.join().await;
                    let mut inner = self_.lock();
                    inner.children.retain(|a| a.pid() != child.pid());
                    join
                })
            }
        }
        futures::future::join_all(waits.into_iter())
            .await
            .into_iter()
            .next()
    }

    /// Waits for any of the children to finished
    pub async fn join_any_child(&self) -> Result<Option<(WasiProcessId, ExitCode)>, Errno> {
        let _guard = WasiProcessWait::new(self);
        let children: Vec<_> = {
            let inner = self.lock();
            inner.children.clone()
        };
        if children.is_empty() {
            return Err(Errno::Child);
        }

        let mut waits = Vec::new();
        for child in children {
            if let Some(process) = self.state.compute.must_upgrade().get_process(child.pid()) {
                let self_ = self.clone();
                waits.push(async move {
                    let join = process.join().await;
                    let mut inner = self_.lock();
                    inner.children.retain(|a| a.pid() != child.pid());
                    (child, join)
                })
            }
        }
        let (child, res) = futures::future::select_all(waits.into_iter().map(|a| Box::pin(a)))
            .await
            .0;

        let code =
            res.unwrap_or_else(|e| e.as_exit_code().unwrap_or_else(|| Errno::Canceled.into()));

        Ok(Some((child.pid(), code)))
    }

    /// Terminate the process and all its threads
    pub fn terminate(&self, exit_code: ExitCode) {
        self.state
            .should_terminate_with_code
            .store(exit_code.raw(), Ordering::Release);
        self.signal_process(Signal::Sigkill);
    }

    /// Terminate the process and wait for all its threads to finish.
    pub async fn terminate_wait(&self, exit_code: ExitCode) {
        self.terminate(exit_code);
        self.join_children().await;
        self.join().await.ok();
    }
}

impl WasiProcessInner {
    /// Checkpoints the process which will cause all other threads to
    /// pause and for the thread and memory state to be saved
    #[cfg(feature = "journal")]
    pub fn checkpoint<M: wasmer_types::MemorySize>(
        process: WasiProcess,
        ctx: FunctionEnvMut<'_, WasiEnv>,
        for_what: ProcessCheckpoint,
    ) -> WasiResult<MaybeCheckpointResult<'_>> {
        // Set the checkpoint flag and then enter the normal processing loop
        {
            // TODO: add set_checkpoint method
            let mut inner = process.lock();
            inner.checkpoint = for_what;
        }

        Self::maybe_checkpoint::<M>(process, ctx)
    }

    /// If a checkpoint has been started this will block the current process
    /// until the checkpoint operation has completed
    #[cfg(feature = "journal")]
    pub fn maybe_checkpoint<M: wasmer_types::MemorySize>(
        process: WasiProcess,
        ctx: FunctionEnvMut<'_, WasiEnv>,
    ) -> WasiResult<MaybeCheckpointResult<'_>> {
        // Enter the lock which will determine if we are in a checkpoint or not

        use bytes::Bytes;
        use wasmer::AsStoreMut;
        use wasmer_types::OnCalledAction;

        use crate::{rewind_ext, WasiError};
        {
            let guard = process.lock();
            if guard.checkpoint == ProcessCheckpoint::Execute {
                // No checkpoint so just carry on
                return Ok(Ok(MaybeCheckpointResult::NotThisTime(ctx)));
            }
            trace!("checkpoint capture");
            drop(guard);
        }

        // Perform the unwind action
        unwind::<M, _>(ctx, move |mut ctx, memory_stack, rewind_stack| {
            // Grab all the globals and serialize them
            let store_data =
                crate::utils::store::capture_instance_snapshot(&mut ctx.as_store_mut())
                    .serialize()
                    .unwrap();
            let memory_stack = memory_stack.freeze();
            let rewind_stack = rewind_stack.freeze();
            let store_data = Bytes::from(store_data);

            tracing::debug!(
                "stack snapshot unwind (memory_stack={}, rewind_stack={}, store_data={})",
                memory_stack.len(),
                rewind_stack.len(),
                store_data.len(),
            );

            // Write our thread state to the snapshot
            let tid = ctx.data().thread.tid();
            if let Err(err) = JournalEffector::save_thread_state::<M>(
                &mut ctx,
                tid,
                memory_stack.clone(),
                rewind_stack.clone(),
                store_data.clone(),
            ) {
                return wasmer_types::OnCalledAction::Trap(err.into());
            }

            let mut guard = process.lock();

            // Wait for the checkpoint to finish (or if we are the last thread
            // to freeze then we have to execute the checksum operation)
            loop {
                if let ProcessCheckpoint::Snapshot { trigger } = guard.checkpoint {
                    ctx.data().thread.set_check_pointing(true);

                    // Now if we are the last thread we also write the memory
                    let is_last_thread = guard.threads.values().all(WasiThread::is_check_pointing);
                    if is_last_thread {
                        if let Err(err) =
                            JournalEffector::save_memory_and_snapshot(&mut ctx, &mut guard, trigger)
                        {
                            process.lock_notify_all();
                            return wasmer_types::OnCalledAction::Trap(err.into());
                        }

                        // Clear the checkpointing flag and notify everyone to wake up
                        ctx.data().thread.set_check_pointing(false);
                        guard.checkpoint = ProcessCheckpoint::Execute;
                        trace!("checkpoint complete");
                        process.lock_notify_all();
                    } else {
                        guard = process.lock_wait(guard);
                    }
                    continue;
                }

                ctx.data().thread.set_check_pointing(false);
                trace!("checkpoint finished");

                // Rewind the stack and carry on
                return match rewind_ext::<M>(&mut ctx, memory_stack, rewind_stack, store_data, None)
                {
                    Errno::Success => OnCalledAction::InvokeAgain,
                    err => {
                        tracing::warn!(
                            "snapshot resumption failed - could not rewind the stack - errno={}",
                            err
                        );
                        OnCalledAction::Trap(Box::new(WasiError::Exit(err.into())))
                    }
                };
            }
        })?;

        Ok(Ok(MaybeCheckpointResult::Unwinding))
    }
}

/// Weak handle to a process.
#[derive(Debug, Clone)]
pub struct WasiProcessHandle {
    process: Weak<State>,
}

impl WasiProcessHandle {
    fn new(process: &Arc<State>) -> Self {
        Self {
            process: Arc::downgrade(process),
        }
    }

    pub fn upgrade(&self) -> Option<WasiProcess> {
        self.process.upgrade().map(|state| WasiProcess { state })
    }
}

// TODO: why do we need this, how is it used?
pub(crate) struct WasiProcessWait {
    waiting: Arc<AtomicU32>,
}

impl WasiProcessWait {
    pub fn new(process: &WasiProcess) -> Self {
        process.state.waiting.fetch_add(1, Ordering::AcqRel);
        Self {
            waiting: process.state.waiting.clone(),
        }
    }
}

impl Drop for WasiProcessWait {
    fn drop(&mut self) {
        self.waiting.fetch_sub(1, Ordering::AcqRel);
    }
}

impl SignalHandlerAbi for WasiProcess {
    fn signal(&self, sig: u8) -> Result<(), SignalDeliveryError> {
        if let Ok(sig) = sig.try_into() {
            self.signal_process(sig);
            Ok(())
        } else {
            Err(SignalDeliveryError)
        }
    }
}
