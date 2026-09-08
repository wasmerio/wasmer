use crate::{WasiEnv, WasiRuntimeError, journal::SnapshotTrigger};
#[cfg(feature = "journal")]
use crate::{WasiResult, journal::JournalEffector, syscalls::do_checkpoint_from_outside, unwind};
use serde::{Deserialize, Serialize};
#[cfg(feature = "journal")]
use std::collections::HashSet;
use std::{
    collections::HashMap,
    convert::TryInto,
    ops::Range,
    sync::{
        Arc, Condvar, Mutex, MutexGuard, RwLock, Weak,
        atomic::{AtomicU32, Ordering},
    },
    task::Waker,
    time::Duration,
};
use tracing::trace;
use wasmer::{FunctionEnvMut, MemoryOps};
use wasmer_types::ModuleHash;
use wasmer_wasix_types::{
    types::Signal,
    wasi::{Errno, ExitCode, Snapshot0Clockid},
    wasix::ThreadStartType,
};

use crate::{
    WasiThread, WasiThreadHandle, WasiThreadId, os::task::signal::WasiSignalInterval,
    syscalls::platform_clock_time_get,
};

use super::{
    TaskStatus,
    backoff::WasiProcessCpuBackoff,
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

pub type LockableWasiProcessInner = Arc<(Mutex<WasiProcessInner>, Condvar)>;
pub(crate) type ChildExitResult = Result<ExitCode, Arc<WasiRuntimeError>>;
pub(crate) type ReapedChild = (WasiProcessId, ChildExitResult);

/// Represents a process running within the compute state
/// TODO: fields should be private and only accessed via methods.
#[derive(Debug, Clone)]
pub struct WasiProcess {
    /// Unique ID of this process
    pub(crate) pid: WasiProcessId,
    /// Hash of the module that this process is using
    pub(crate) module_hash: ModuleHash,
    /// List of all the children spawned from this thread
    pub(crate) parent: Option<Weak<RwLock<WasiProcessInner>>>,
    /// The inner protected region of the process with a conditional
    /// variable that is used for coordination such as snapshots.
    pub(crate) inner: LockableWasiProcessInner,
    /// Reference back to the compute engine
    // TODO: remove this reference, access should happen via separate state instead
    // (we don't want cyclical references)
    pub(crate) compute: WasiControlPlaneHandle,
    /// Reference to the exit code for the main thread
    pub(crate) finished: Arc<OwnedTaskStatus>,
    /// Number of threads waiting for children to exit
    pub(crate) waiting: Arc<AtomicU32>,
    /// Number of tokens that are currently active and thus
    /// the exponential backoff of CPU is halted (as in CPU
    /// is allowed to run freely)
    pub(crate) cpu_run_tokens: Arc<AtomicU32>,
}

/// Represents a freeze of all threads to perform some action
/// on the total state-machine. This is normally done for
/// things like snapshots which require the memory to remain
/// stable while it performs a diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WasiProcessCheckpoint {
    /// No checkpoint will take place and the process
    /// should just execute as per normal
    Execute,
    /// The process needs to take a snapshot of the
    /// memory and state-machine
    Snapshot { trigger: SnapshotTrigger },
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MemorySnapshotRegion {
    pub start: u64,
    pub end: u64,
}

impl From<Range<u64>> for MemorySnapshotRegion {
    fn from(value: Range<u64>) -> Self {
        Self {
            start: value.start,
            end: value.end,
        }
    }
}

#[allow(clippy::from_over_into)]
impl Into<Range<u64>> for MemorySnapshotRegion {
    fn into(self) -> Range<u64> {
        self.start..self.end
    }
}

// TODO: fields should be private and only accessed via methods.
#[derive(Debug)]
pub struct WasiProcessInner {
    /// Unique ID of this process
    pub pid: WasiProcessId,
    /// Number of threads waiting for children to exit
    pub(crate) waiting: Arc<AtomicU32>,
    /// The threads that make up this process
    pub threads: HashMap<WasiThreadId, WasiThread>,
    /// Number of threads running for this process
    pub thread_count: u32,
    /// Signals that will be triggered at specific intervals
    pub signal_intervals: HashMap<Signal, WasiSignalInterval>,
    /// List of all the children spawned from this thread
    pub children: Vec<WasiProcess>,
    /// Represents a checkpoint which blocks all the threads
    /// and then executes some maintenance action
    pub checkpoint: WasiProcessCheckpoint,
    /// If true then the journaling will be disabled after the
    /// next snapshot is taken
    pub disable_journaling_after_checkpoint: bool,
    /// If true then the process will stop running after the
    /// next snapshot is taken
    pub stop_running_after_checkpoint: bool,
    /// List of situations that the process will checkpoint on
    #[cfg(feature = "journal")]
    pub snapshot_on: HashSet<SnapshotTrigger>,
    /// Any wakers waiting on this process (for example for a checkpoint)
    pub wakers: Vec<Waker>,
    /// If true then the process has started cleaning up
    pub cleanup_started: bool,
    /// Shared process memory.
    pub memory: Option<MemoryOps>,
    /// The snapshot memory significantly reduce the amount of
    /// duplicate entries in the journal for memory that has not changed
    #[cfg(feature = "journal")]
    pub snapshot_memory_hash: HashMap<MemorySnapshotRegion, u64>,
    /// Represents all the backoff properties for this process
    /// which will be used to determine if the CPU should be
    /// throttled or not
    pub(super) backoff: WasiProcessCpuBackoff,
}

pub enum MaybeCheckpointResult<'a> {
    NotThisTime(FunctionEnvMut<'a, WasiEnv>),
    Unwinding,
}

impl WasiProcessInner {
    /// Checkpoints the process which will cause all other threads to
    /// pause and for the thread and memory state to be saved
    #[cfg(feature = "journal")]
    pub fn checkpoint<M: wasmer_types::MemorySize>(
        inner: LockableWasiProcessInner,
        ctx: FunctionEnvMut<'_, WasiEnv>,
        for_what: WasiProcessCheckpoint,
    ) -> WasiResult<MaybeCheckpointResult<'_>> {
        // Set the checkpoint flag and then enter the normal processing loop
        {
            let mut guard = inner.0.lock().unwrap();
            guard.checkpoint = for_what;
            for waker in guard.wakers.drain(..) {
                waker.wake();
            }
            inner.1.notify_all();
        }

        Self::maybe_checkpoint::<M>(inner, ctx)
    }

    /// If a checkpoint has been started this will block the current process
    /// until the checkpoint operation has completed
    #[cfg(feature = "journal")]
    pub fn maybe_checkpoint<M: wasmer_types::MemorySize>(
        inner: LockableWasiProcessInner,
        ctx: FunctionEnvMut<'_, WasiEnv>,
    ) -> WasiResult<MaybeCheckpointResult<'_>> {
        // Enter the lock which will determine if we are in a checkpoint or not

        use bytes::Bytes;
        use wasmer::AsStoreMut;
        use wasmer_types::OnCalledAction;

        use crate::{WasiError, os::task::thread::RewindResultType, rewind_ext};
        let guard = inner.0.lock().unwrap();
        if guard.checkpoint == WasiProcessCheckpoint::Execute {
            // No checkpoint so just carry on
            return Ok(Ok(MaybeCheckpointResult::NotThisTime(ctx)));
        }
        trace!("checkpoint capture");
        drop(guard);

        // Perform the unwind action
        let thread_layout = ctx.data().thread.memory_layout().clone();
        unwind::<M, _>(ctx, move |mut ctx, memory_stack, rewind_stack| {
            // Grab all the globals and serialize them
            let store_data = crate::utils::store::capture_store_snapshot(&mut ctx.as_store_mut())
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
            let thread_start = ctx.data().thread.thread_start_type();
            let tid = ctx.data().thread.tid();
            if let Err(err) = JournalEffector::save_thread_state::<M>(
                &mut ctx,
                tid,
                memory_stack.clone(),
                rewind_stack.clone(),
                store_data.clone(),
                thread_start,
                thread_layout,
            ) {
                return wasmer_types::OnCalledAction::Trap(err.into());
            }

            let mut guard = inner.0.lock().unwrap();

            // Wait for the checkpoint to finish (or if we are the last thread
            // to freeze then we have to execute the checksum operation)
            loop {
                if let WasiProcessCheckpoint::Snapshot { trigger } = guard.checkpoint {
                    ctx.data().thread.set_checkpointing(true);

                    // Now if we are the last thread we also write the memory
                    let is_last_thread = guard
                        .threads
                        .values()
                        .all(|t| t.is_check_pointing() || t.is_deep_sleeping());
                    if is_last_thread {
                        if let Err(err) =
                            JournalEffector::save_memory_and_snapshot(&mut ctx, &mut guard, trigger)
                        {
                            inner.1.notify_all();
                            return wasmer_types::OnCalledAction::Trap(err.into());
                        }

                        // Clear the checkpointing flag and notify everyone to wake up
                        ctx.data().thread.set_checkpointing(false);
                        trace!("checkpoint complete");
                        if guard.disable_journaling_after_checkpoint {
                            ctx.data_mut().enable_journal = false;
                        }
                        guard.checkpoint = WasiProcessCheckpoint::Execute;
                        for waker in guard.wakers.drain(..) {
                            waker.wake();
                        }
                        inner.1.notify_all();
                    } else {
                        guard = inner.1.wait(guard).unwrap();
                    }
                    continue;
                }

                ctx.data().thread.set_checkpointing(false);
                trace!("checkpoint finished");

                if guard.stop_running_after_checkpoint {
                    trace!("will stop running now");
                    // Need to stop recording journal events so we don't also record the
                    // thread and process exit events
                    ctx.data_mut().enable_journal = false;
                    return OnCalledAction::Finish;
                }

                // Rewind the stack and carry on
                return match rewind_ext::<M>(
                    &mut ctx,
                    Some(memory_stack),
                    rewind_stack,
                    store_data,
                    RewindResultType::RewindWithoutResult,
                ) {
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

    // Execute any checkpoints that can be executed while outside of the WASM process
    #[cfg(not(feature = "journal"))]
    pub fn do_checkpoints_from_outside(_ctx: &mut FunctionEnvMut<'_, WasiEnv>) {}

    // Execute any checkpoints that can be executed while outside of the WASM process
    #[cfg(feature = "journal")]
    pub fn do_checkpoints_from_outside(ctx: &mut FunctionEnvMut<'_, WasiEnv>) {
        let inner = ctx.data().process.inner.clone();
        let mut guard = inner.0.lock().unwrap();

        // Wait for the checkpoint to finish (or if we are the last thread
        // to freeze then we have to execute the checksum operation)
        while let WasiProcessCheckpoint::Snapshot { trigger } = guard.checkpoint {
            ctx.data().thread.set_checkpointing(true);

            // Now if we are the last thread we also write the memory
            let is_last_thread = guard
                .threads
                .values()
                .all(|t| t.is_check_pointing() || t.is_deep_sleeping());
            if is_last_thread {
                if let Err(err) =
                    JournalEffector::save_memory_and_snapshot(ctx, &mut guard, trigger)
                {
                    inner.1.notify_all();
                    tracing::error!("failed to snapshot memory and threads - {}", err);
                    return;
                }

                // Clear the checkpointing flag and notify everyone to wake up
                ctx.data().thread.set_checkpointing(false);
                trace!("checkpoint complete");
                if guard.disable_journaling_after_checkpoint {
                    ctx.data_mut().enable_journal = false;
                }
                guard.checkpoint = WasiProcessCheckpoint::Execute;
                for waker in guard.wakers.drain(..) {
                    waker.wake();
                }
                inner.1.notify_all();
            } else {
                guard = inner.1.wait(guard).unwrap();
            }
            continue;
        }

        ctx.data().thread.set_checkpointing(false);
        trace!("checkpoint finished");
    }
}

// TODO: why do we need this, how is it used?
pub(crate) struct WasiProcessWait {
    waiting: Arc<AtomicU32>,
}

impl WasiProcessWait {
    pub fn new(process: &WasiProcess) -> Self {
        process.waiting.fetch_add(1, Ordering::AcqRel);
        Self {
            waiting: process.waiting.clone(),
        }
    }
}

impl Drop for WasiProcessWait {
    fn drop(&mut self) {
        self.waiting.fetch_sub(1, Ordering::AcqRel);
    }
}

impl WasiProcess {
    pub fn new(pid: WasiProcessId, module_hash: ModuleHash, plane: WasiControlPlaneHandle) -> Self {
        let max_cpu_backoff_time = plane
            .upgrade()
            .and_then(|p| p.config().enable_exponential_cpu_backoff)
            .unwrap_or(Duration::from_secs(30));
        let max_cpu_cool_off_time = Duration::from_millis(500);

        let waiting = Arc::new(AtomicU32::new(0));
        let inner = Arc::new((
            Mutex::new(WasiProcessInner {
                pid,
                threads: Default::default(),
                thread_count: Default::default(),
                signal_intervals: Default::default(),
                children: Default::default(),
                checkpoint: WasiProcessCheckpoint::Execute,
                wakers: Default::default(),
                cleanup_started: false,
                memory: Default::default(),
                waiting: waiting.clone(),
                #[cfg(feature = "journal")]
                snapshot_on: Default::default(),
                #[cfg(feature = "journal")]
                snapshot_memory_hash: Default::default(),
                disable_journaling_after_checkpoint: false,
                stop_running_after_checkpoint: false,
                backoff: WasiProcessCpuBackoff::new(max_cpu_backoff_time, max_cpu_cool_off_time),
            }),
            Condvar::new(),
        ));

        #[derive(Debug)]
        struct SignalHandler(LockableWasiProcessInner);
        impl SignalHandlerAbi for SignalHandler {
            fn signal(&self, signal: u8) -> Result<(), SignalDeliveryError> {
                if let Ok(signal) = signal.try_into() {
                    signal_process_internal(&self.0, signal);
                    Ok(())
                } else {
                    Err(SignalDeliveryError)
                }
            }
        }

        WasiProcess {
            pid,
            module_hash,
            parent: None,
            compute: plane,
            inner: inner.clone(),
            finished: Arc::new(
                OwnedTaskStatus::new(TaskStatus::Pending)
                    .with_signal_handler(Arc::new(SignalHandler(inner))),
            ),
            waiting,
            cpu_run_tokens: Arc::new(AtomicU32::new(0)),
        }
    }

    /// Tries to start the cleanup process, returns true if this is the first
    /// thread to start the cleanup.
    pub fn try_start_cleanup(&self) -> bool {
        let mut guard = self.inner.0.lock().unwrap();
        if guard.cleanup_started {
            false
        } else {
            guard.cleanup_started = true;
            true
        }
    }

    pub(super) fn set_pid(&mut self, pid: WasiProcessId) {
        self.pid = pid;
    }

    /// Gets the process ID of this process
    pub fn pid(&self) -> WasiProcessId {
        self.pid
    }

    /// Gets the process ID of the parent process
    pub fn ppid(&self) -> WasiProcessId {
        self.parent
            .iter()
            .filter_map(|parent| parent.upgrade())
            .map(|parent| parent.read().unwrap().pid)
            .next()
            .unwrap_or(WasiProcessId(0))
    }

    /// Gains access to the process internals
    // TODO: Make this private, all inner access should be exposed with methods.
    pub fn lock(&self) -> MutexGuard<'_, WasiProcessInner> {
        self.inner.0.lock().unwrap()
    }

    /// Creates a thread and returns it
    pub fn new_thread(
        &self,
        layout: WasiMemoryLayout,
        start: ThreadStartType,
    ) -> Result<WasiThreadHandle, ControlPlaneError> {
        let control_plane = self.compute.must_upgrade();

        // Determine if its the main thread or not
        let is_main = matches!(start, ThreadStartType::MainThread);

        // Generate a new process ID (this is because the process ID and thread ID
        // address space must not overlap in libc). For the main process the TID=PID
        let tid: WasiThreadId = if is_main {
            self.pid().raw().into()
        } else {
            let tid: u32 = control_plane.generate_id()?.into();
            tid.into()
        };

        self.new_thread_with_id(layout, start, tid)
    }

    /// Creates a thread and returns it
    pub fn new_thread_with_id(
        &self,
        layout: WasiMemoryLayout,
        start: ThreadStartType,
        tid: WasiThreadId,
    ) -> Result<WasiThreadHandle, ControlPlaneError> {
        let control_plane = self.compute.must_upgrade();
        let task_count_guard = control_plane.register_task()?;

        let is_main = matches!(start, ThreadStartType::MainThread);

        // The wait finished should be the process version if its the main thread
        let mut inner = self.inner.0.lock().unwrap();
        let finished = if is_main {
            self.finished.clone()
        } else {
            Arc::new(OwnedTaskStatus::default())
        };

        // Insert the thread into the pool
        let ctrl = WasiThread::new(
            self.pid(),
            tid,
            is_main,
            finished,
            task_count_guard,
            layout,
            start,
        );
        inner.threads.insert(tid, ctrl.clone());
        inner.thread_count += 1;

        Ok(WasiThreadHandle::new(ctrl, &self.inner))
    }

    pub fn all_threads(&self) -> Vec<WasiThreadId> {
        let inner = self.inner.0.lock().unwrap();
        inner.threads.keys().cloned().collect()
    }

    /// Gets a reference to a particular thread
    pub fn get_thread(&self, tid: &WasiThreadId) -> Option<WasiThread> {
        let inner = self.inner.0.lock().unwrap();
        inner.threads.get(tid).cloned()
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

        let inner = self.inner.0.lock().unwrap();

        wake_atomic_waiters(&inner, signal);
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
        signal_process_internal(&self.inner, signal);
    }

    /// Registers the shared memory used by this process.
    pub fn register_memory(&self, memory: impl Into<MemoryOps>) {
        let mut inner = self.inner.0.lock().unwrap();
        inner.memory = Some(memory.into());
    }

    /// Takes a snapshot of the process and disables journaling returning
    /// a future that can be waited on for the snapshot to complete
    ///
    /// Note: If you ignore the returned future the checkpoint will still
    /// occur but it will execute asynchronously
    pub fn snapshot_and_disable_journaling(
        &self,
        trigger: SnapshotTrigger,
    ) -> std::pin::Pin<Box<dyn futures::Future<Output = ()> + Send + Sync>> {
        let mut guard = self.inner.0.lock().unwrap();
        guard.disable_journaling_after_checkpoint = true;
        guard.checkpoint = WasiProcessCheckpoint::Snapshot { trigger };
        self.wait_for_checkpoint_finish()
    }

    /// Takes a snapshot of the process and shuts it down after the snapshot
    /// is taken.
    ///
    /// Note: If you ignore the returned future the checkpoint will still
    /// occur but it will execute asynchronously
    pub fn snapshot_and_stop(
        &self,
        trigger: SnapshotTrigger,
    ) -> std::pin::Pin<Box<dyn futures::Future<Output = ()> + Send + Sync>> {
        let mut guard = self.inner.0.lock().unwrap();
        guard.stop_running_after_checkpoint = true;
        guard.checkpoint = WasiProcessCheckpoint::Snapshot { trigger };
        self.wait_for_checkpoint_finish()
    }

    /// Takes a snapshot of the process
    ///
    /// Note: If you ignore the returned future the checkpoint will still
    /// occur but it will execute asynchronously
    pub fn snapshot(
        &self,
        trigger: SnapshotTrigger,
    ) -> std::pin::Pin<Box<dyn futures::Future<Output = ()> + Send + Sync>> {
        let mut guard = self.inner.0.lock().unwrap();
        guard.checkpoint = WasiProcessCheckpoint::Snapshot { trigger };
        self.wait_for_checkpoint_finish()
    }

    /// Disables the journaling functionality
    pub fn disable_journaling_after_checkpoint(&self) {
        let mut guard = self.inner.0.lock().unwrap();
        guard.disable_journaling_after_checkpoint = true;
    }

    /// Stop running once a checkpoint is taken
    pub fn stop_running_after_checkpoint(&self) {
        let mut guard = self.inner.0.lock().unwrap();
        guard.stop_running_after_checkpoint = true;
    }

    /// Wait for the checkout process to finish
    #[cfg(not(feature = "journal"))]
    pub fn wait_for_checkpoint(
        &self,
    ) -> std::pin::Pin<Box<dyn futures::Future<Output = ()> + Send + Sync>> {
        Box::pin(std::future::pending())
    }

    /// Wait for the checkout process to finish
    #[cfg(feature = "journal")]
    pub fn wait_for_checkpoint(
        &self,
    ) -> std::pin::Pin<Box<dyn futures::Future<Output = ()> + Send + Sync>> {
        use futures::Future;
        use std::{
            pin::Pin,
            task::{Context, Poll},
        };

        struct Poller {
            inner: LockableWasiProcessInner,
        }
        impl Future for Poller {
            type Output = ();
            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                let mut guard = self.inner.0.lock().unwrap();
                if !matches!(guard.checkpoint, WasiProcessCheckpoint::Execute) {
                    return Poll::Ready(());
                }
                if !guard.wakers.iter().any(|w| w.will_wake(cx.waker())) {
                    guard.wakers.push(cx.waker().clone());
                }
                Poll::Pending
            }
        }
        Box::pin(Poller {
            inner: self.inner.clone(),
        })
    }

    /// Wait for the checkout process to finish
    #[cfg(not(feature = "journal"))]
    pub fn wait_for_checkpoint_finish(
        &self,
    ) -> std::pin::Pin<Box<dyn futures::Future<Output = ()> + Send + Sync>> {
        Box::pin(std::future::pending())
    }

    /// Wait for the checkout process to finish
    #[cfg(feature = "journal")]
    pub fn wait_for_checkpoint_finish(
        &self,
    ) -> std::pin::Pin<Box<dyn futures::Future<Output = ()> + Send + Sync>> {
        use futures::Future;
        use std::{
            pin::Pin,
            task::{Context, Poll},
        };

        struct Poller {
            inner: LockableWasiProcessInner,
        }
        impl Future for Poller {
            type Output = ();
            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                let mut guard = self.inner.0.lock().unwrap();
                if matches!(guard.checkpoint, WasiProcessCheckpoint::Execute) {
                    return Poll::Ready(());
                }
                if !guard.wakers.iter().any(|w| w.will_wake(cx.waker())) {
                    guard.wakers.push(cx.waker().clone());
                }
                Poll::Pending
            }
        }
        Box::pin(Poller {
            inner: self.inner.clone(),
        })
    }

    /// Signals one of the threads every interval
    pub fn signal_interval(&self, signal: Signal, interval: Option<Duration>, repeat: bool) {
        let mut inner = self.inner.0.lock().unwrap();

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
        let inner = self.inner.0.lock().unwrap();
        inner.thread_count
    }

    /// Waits until the process is finished.
    pub async fn join(&self) -> Result<ExitCode, Arc<WasiRuntimeError>> {
        let _guard = WasiProcessWait::new(self);
        self.finished.await_termination().await
    }

    /// Attempts to join on the process
    pub fn try_join(&self) -> Option<Result<ExitCode, Arc<WasiRuntimeError>>> {
        self.finished.status().into_finished()
    }

    /// Claims one finished child from this process.
    pub(crate) fn try_reap_child(
        &self,
        pid: Option<WasiProcessId>,
    ) -> Result<Option<ReapedChild>, Errno> {
        let mut inner = self.inner.0.lock().unwrap();
        if inner.children.is_empty() {
            return Err(Errno::Child);
        }

        let finished = match pid {
            Some(pid) => {
                let index = inner
                    .children
                    .iter()
                    .position(|child| child.pid == pid)
                    .ok_or(Errno::Child)?;
                inner.children[index]
                    .try_join()
                    .map(|result| (index, result))
            }
            None => inner
                .children
                .iter()
                .enumerate()
                .find_map(|(index, child)| child.try_join().map(|result| (index, result))),
        };

        let Some((index, result)) = finished else {
            return Ok(None);
        };
        let child = inner.children.remove(index);
        Ok(Some((child.pid, result)))
    }

    pub(crate) fn child_exit_code(result: ChildExitResult) -> ExitCode {
        result.unwrap_or_else(|error| {
            error
                .as_exit_code()
                .unwrap_or_else(|| Errno::Canceled.into())
        })
    }

    /// Waits for and claims a specific child process.
    pub(crate) async fn join_child(
        &self,
        pid: WasiProcessId,
    ) -> Result<(WasiProcessId, ExitCode), Errno> {
        let _guard = WasiProcessWait::new(self);
        loop {
            if let Some((pid, result)) = self.try_reap_child(Some(pid))? {
                return Ok((pid, Self::child_exit_code(result)));
            }

            let child = {
                let inner = self.inner.0.lock().unwrap();
                inner
                    .children
                    .iter()
                    .find(|child| child.pid == pid)
                    .cloned()
                    .ok_or(Errno::Child)?
            };
            let _ = child.join().await;
        }
    }

    /// Waits for all the children to be finished
    pub async fn join_children(&mut self) -> Option<Result<ExitCode, Arc<WasiRuntimeError>>> {
        let _guard = WasiProcessWait::new(self);
        let children: Vec<_> = {
            let inner = self.inner.0.lock().unwrap();
            inner.children.clone()
        };
        if children.is_empty() {
            return None;
        }

        futures::future::join_all(children.iter().map(WasiProcess::join)).await;

        let mut first = None;
        for child in children {
            if let Ok(Some((_, result))) = self.try_reap_child(Some(child.pid)) {
                first = first.or(Some(result));
            }
        }
        first
    }

    /// Waits for any of the children to finish
    pub async fn join_any_child(&self) -> Result<Option<(WasiProcessId, ExitCode)>, Errno> {
        let _guard = WasiProcessWait::new(self);
        loop {
            if let Some((pid, result)) = self.try_reap_child(None)? {
                return Ok(Some((pid, Self::child_exit_code(result))));
            }

            let children = {
                let inner = self.inner.0.lock().unwrap();
                if inner.children.is_empty() {
                    return Err(Errno::Child);
                }
                inner.children.clone()
            };

            let waits = children
                .iter()
                .map(|child| Box::pin(child.join()))
                .collect::<Vec<_>>();
            let _ = futures::future::select_all(waits).await;
        }
    }

    /// Terminate the process and all its threads
    pub fn terminate(&self, exit_code: ExitCode) {
        let pid = self.pid;
        tracing::trace!(%pid, %exit_code, "process-terminate");
        // FIXME: this is wrong, threads might still be running!
        // Need special logic for the main thread.
        let guard = self.inner.0.lock().unwrap();
        for thread in guard.threads.values() {
            thread.set_status_finished(Ok(exit_code))
        }
    }
}

/// Signals all the threads in this process
fn signal_process_internal(process: &LockableWasiProcessInner, signal: Signal) {
    #[allow(unused_mut)]
    let mut guard = process.0.lock().unwrap();
    let pid = guard.pid;
    tracing::trace!(%pid, "signal-process({:?})", signal);

    // If the snapshot on ctrl-c is currently registered then we need
    // to take a snapshot and exit
    #[cfg(feature = "journal")]
    {
        if signal == Signal::Sigint
            && (guard.snapshot_on.contains(&SnapshotTrigger::Sigint)
                || guard.snapshot_on.remove(&SnapshotTrigger::FirstSigint))
        {
            drop(guard);

            tracing::debug!(%pid, "snapshot-on-interrupt-signal");

            do_checkpoint_from_outside(
                process,
                WasiProcessCheckpoint::Snapshot {
                    trigger: SnapshotTrigger::Sigint,
                },
            );
            return;
        };
    }

    // Check if there are subprocesses that will receive this signal
    // instead of this process
    if guard.waiting.load(Ordering::Acquire) > 0 {
        let mut triggered = false;
        for child in guard.children.iter() {
            child.signal_process(signal);
            triggered = true;
        }
        if triggered {
            return;
        }
    }

    // Otherwise just send the signal to all the threads
    wake_atomic_waiters(&guard, signal);
    for thread in guard.threads.values() {
        thread.signal(signal);
    }
}

fn wake_atomic_waiters(process: &WasiProcessInner, signal: Signal) {
    let Some(memory) = &process.memory else {
        return;
    };

    if signal == Signal::Sigkill {
        // On kill, disable atomics to prevent threads from resuming.
        // NOTE: disable_atomics also wakes all current waiters.
        if let Err(err) = memory.disable_atomics() {
            tracing::trace!(
                pid=%process.pid,
                error = &err as &dyn std::error::Error,
                "failed to wake atomic waiters"
            );
        }
    }

    // TODO: Should other signals also wake up waiters?
    // We have low confidence this is useful outside the kill path.
    // SEE https://github.com/wasmerio/wasmer/pull/6536
    //
    // Atomic wait wakeups are memory-wide, so only use them for signals
    // that should interrupt or terminate execution anyway.
    // if matches!(
    //     signal,
    //     Signal::Sigkill
    //         | Signal::Sigterm
    //         | Signal::Sigabrt
    //         | Signal::Sigquit
    //         | Signal::Sigint
    //         | Signal::Sigstop
    //         | Signal::Sigpipe
    //         | Signal::Sigwakeup
    // ) {
    //    memory.wake_all_atomic_waiters();
    // }
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

#[cfg(test)]
mod tests {
    use std::{future::Future, sync::Arc, time::Duration};

    use futures::FutureExt;
    use tokio::sync::Barrier;

    use super::*;
    use crate::os::task::control_plane::WasiControlPlane;

    fn parent_with_children(count: usize) -> (WasiProcess, Vec<WasiProcess>) {
        let control_plane = WasiControlPlane::default();
        let parent = control_plane.new_process(ModuleHash::random()).unwrap();
        let children = (0..count)
            .map(|_| control_plane.new_process(ModuleHash::random()).unwrap())
            .collect::<Vec<_>>();
        parent.lock().children.extend(children.iter().cloned());
        (parent, children)
    }

    fn finish(child: &WasiProcess, code: u16) {
        child.finished.set_finished(Ok(ExitCode::from(code)));
    }

    fn fail(child: &WasiProcess) {
        child
            .finished
            .set_finished(Err(Arc::new(WasiRuntimeError::Anyhow(Arc::new(
                anyhow::anyhow!("child failed"),
            )))));
    }

    async fn wait_for_waiters(process: &WasiProcess, expected: u32) {
        for _ in 0..10_000 {
            if process.waiting.load(Ordering::Acquire) == expected {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("expected {expected} waiters");
    }

    async fn gated_specific(
        process: WasiProcess,
        pid: WasiProcessId,
        barrier: Arc<Barrier>,
    ) -> Result<WasiProcessId, Errno> {
        barrier.wait().await;
        process.join_child(pid).await.map(|(pid, _)| pid)
    }

    async fn gated_any(
        process: WasiProcess,
        barrier: Arc<Barrier>,
    ) -> Result<WasiProcessId, Errno> {
        barrier.wait().await;
        process
            .join_any_child()
            .await
            .map(|result| result.expect("a successful wait returns a child").0)
    }

    async fn run_race<A, B>(
        parent: &WasiProcess,
        child: &WasiProcess,
        first: A,
        second: B,
        barrier: Arc<Barrier>,
    ) -> (Result<WasiProcessId, Errno>, Result<WasiProcessId, Errno>)
    where
        A: Future<Output = Result<WasiProcessId, Errno>>,
        B: Future<Output = Result<WasiProcessId, Errno>>,
    {
        let release = async {
            barrier.wait().await;
            wait_for_waiters(parent, 2).await;
            finish(child, 17);
        };
        let (first, second, ()) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(first, second, release)
        })
        .await
        .expect("child wait race timed out");
        (first, second)
    }

    fn assert_one_reaper(
        pid: WasiProcessId,
        first: Result<WasiProcessId, Errno>,
        second: Result<WasiProcessId, Errno>,
    ) {
        let results = [first, second];
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Ok(result_pid) if *result_pid == pid))
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(Errno::Child)))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn no_or_unknown_child_is_not_waitable() {
        let (parent, _) = parent_with_children(0);
        let unknown = WasiProcessId::from(u32::MAX - 1);

        assert!(matches!(parent.try_reap_child(None), Err(Errno::Child)));
        assert!(matches!(
            parent.try_reap_child(Some(unknown)),
            Err(Errno::Child)
        ));
        assert!(matches!(
            parent.join_child(unknown).await,
            Err(Errno::Child)
        ));
        assert!(matches!(parent.join_any_child().await, Err(Errno::Child)));
    }

    #[test]
    fn running_children_remain_registered() {
        let (parent, children) = parent_with_children(2);

        assert!(
            parent
                .try_reap_child(Some(children[0].pid()))
                .unwrap()
                .is_none()
        );
        assert!(parent.try_reap_child(None).unwrap().is_none());
        assert_eq!(parent.lock().children.len(), 2);
    }

    #[test]
    fn finished_status_is_claimed_once() {
        let (parent, children) = parent_with_children(1);
        finish(&children[0], 7);

        let (pid, status) = parent.try_reap_child(None).unwrap().unwrap();
        assert_eq!(pid, children[0].pid());
        assert_eq!(status.unwrap().raw(), 7);
        assert!(matches!(parent.try_reap_child(None), Err(Errno::Child)));
    }

    #[tokio::test]
    async fn parent_children_are_the_only_waitable_processes() {
        let control_plane = WasiControlPlane::default();
        let parent = control_plane.new_process(ModuleHash::random()).unwrap();
        let child = control_plane.new_process(ModuleHash::random()).unwrap();
        let outsider = control_plane.new_process(ModuleHash::random()).unwrap();
        parent.lock().children.push(child.clone());
        finish(&outsider, 8);

        assert!(matches!(
            parent.try_reap_child(Some(outsider.pid())),
            Err(Errno::Child)
        ));
        assert!(matches!(
            parent.join_child(outsider.pid()).await,
            Err(Errno::Child)
        ));
        assert_eq!(parent.lock().children.len(), 1);
    }

    #[tokio::test]
    async fn child_waits_do_not_need_the_control_plane() {
        let (parent, children) = parent_with_children(1);
        finish(&children[0], 9);

        let (pid, code) = parent.join_any_child().await.unwrap().unwrap();
        assert_eq!(pid, children[0].pid());
        assert_eq!(code.raw(), 9);
    }

    #[tokio::test]
    async fn runtime_errors_use_one_exit_code() {
        let (specific_parent, specific_children) = parent_with_children(1);
        fail(&specific_children[0]);
        let (_, specific_code) = specific_parent
            .join_child(specific_children[0].pid())
            .await
            .unwrap();

        let (any_parent, any_children) = parent_with_children(1);
        fail(&any_children[0]);
        let (_, any_code) = any_parent.join_any_child().await.unwrap().unwrap();

        let expected: ExitCode = Errno::Canceled.into();
        assert_eq!(specific_code, expected);
        assert_eq!(any_code, expected);
    }

    #[tokio::test]
    async fn dropped_pending_wait_keeps_the_child_registered() {
        let (parent, children) = parent_with_children(1);
        let pid = children[0].pid();

        assert!(parent.join_child(pid).now_or_never().is_none());
        assert_eq!(parent.waiting.load(Ordering::Acquire), 0);
        assert_eq!(children[0].waiting.load(Ordering::Acquire), 0);
        assert_eq!(parent.lock().children.len(), 1);

        finish(&children[0], 10);
        assert!(parent.try_reap_child(Some(pid)).unwrap().is_some());
    }

    #[tokio::test]
    async fn specific_waiters_race_for_one_status() {
        let (parent, children) = parent_with_children(1);
        let pid = children[0].pid();
        let barrier = Arc::new(Barrier::new(3));
        let results = run_race(
            &parent,
            &children[0],
            gated_specific(parent.clone(), pid, barrier.clone()),
            gated_specific(parent.clone(), pid, barrier.clone()),
            barrier,
        )
        .await;

        assert_one_reaper(pid, results.0, results.1);
    }

    #[tokio::test]
    async fn any_waiters_race_for_one_status() {
        let (parent, children) = parent_with_children(1);
        let pid = children[0].pid();
        let barrier = Arc::new(Barrier::new(3));
        let results = run_race(
            &parent,
            &children[0],
            gated_any(parent.clone(), barrier.clone()),
            gated_any(parent.clone(), barrier.clone()),
            barrier,
        )
        .await;

        assert_one_reaper(pid, results.0, results.1);
    }

    #[tokio::test]
    async fn specific_and_any_waiters_race_for_one_status() {
        let (parent, children) = parent_with_children(1);
        let pid = children[0].pid();
        let barrier = Arc::new(Barrier::new(3));
        let results = run_race(
            &parent,
            &children[0],
            gated_specific(parent.clone(), pid, barrier.clone()),
            gated_any(parent.clone(), barrier.clone()),
            barrier,
        )
        .await;

        assert_one_reaper(pid, results.0, results.1);
    }

    #[tokio::test]
    async fn any_wait_and_join_children_race_for_one_status() {
        let (parent, children) = parent_with_children(1);
        let pid = children[0].pid();
        let barrier = Arc::new(Barrier::new(3));
        let join_all = {
            let mut process = parent.clone();
            let barrier = barrier.clone();
            async move {
                barrier.wait().await;
                match process.join_children().await {
                    Some(_) => Ok(pid),
                    None => Err(Errno::Child),
                }
            }
        };
        let results = run_race(
            &parent,
            &children[0],
            gated_any(parent.clone(), barrier.clone()),
            join_all,
            barrier,
        )
        .await;

        assert_one_reaper(pid, results.0, results.1);
    }

    #[tokio::test]
    async fn simultaneous_children_go_to_distinct_any_waiters() {
        let (parent, children) = parent_with_children(2);
        let barrier = Arc::new(Barrier::new(3));
        let first = gated_any(parent.clone(), barrier.clone());
        let second = gated_any(parent.clone(), barrier.clone());
        let release = async {
            barrier.wait().await;
            wait_for_waiters(&parent, 2).await;
            finish(&children[0], 21);
            finish(&children[1], 22);
        };
        let (first, second, ()) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(first, second, release)
        })
        .await
        .expect("two-child wait race timed out");

        let mut pids = [first.unwrap(), second.unwrap()];
        pids.sort();
        let mut expected = [children[0].pid(), children[1].pid()];
        expected.sort();
        assert_eq!(pids, expected);
        assert!(parent.lock().children.is_empty());
    }
}
