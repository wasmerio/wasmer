use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
    sync::{Arc, Condvar, Mutex, Weak},
    task::Waker,
};

use bytes::{Bytes, BytesMut};
use wasmer::{ExportError, InstantiationError, MemoryError};
use wasmer_wasix_types::{
    types::Signal,
    wasi::{Errno, ExitCode},
    wasix::ThreadStartType,
};

use crate::{
    os::task::process::{WasiProcessId, WasiProcessInner},
    syscalls::HandleRewindType,
    WasiRuntimeError,
};

use super::{
    control_plane::TaskCountGuard,
    task_join_handle::{OwnedTaskStatus, TaskJoinHandle},
};

/// Represents the ID of a WASI thread
#[derive(Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct WasiThreadId(u32);

impl WasiThreadId {
    pub fn raw(&self) -> u32 {
        self.0
    }

    pub fn inc(&mut self) -> WasiThreadId {
        let ret = *self;
        self.0 += 1;
        ret
    }
}

impl From<i32> for WasiThreadId {
    fn from(id: i32) -> Self {
        Self(id as u32)
    }
}

impl From<WasiThreadId> for i32 {
    fn from(val: WasiThreadId) -> Self {
        val.0 as i32
    }
}

impl From<u32> for WasiThreadId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}

impl From<WasiThreadId> for u32 {
    fn from(t: WasiThreadId) -> u32 {
        t.0
    }
}

impl std::fmt::Display for WasiThreadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Debug for WasiThreadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Represents a linked list of stack snapshots
#[derive(Debug, Clone)]
struct ThreadSnapshot {
    call_stack: Bytes,
    store_data: Bytes,
}

/// Represents a linked list of stack snapshots
#[derive(Debug, Clone, Default)]
pub struct ThreadStack {
    memory_stack: Vec<u8>,
    memory_stack_corrected: Vec<u8>,
    snapshots: HashMap<u128, ThreadSnapshot>,
    next: Option<Box<ThreadStack>>,
}

/// Represents a running thread which allows a joiner to
/// wait for the thread to exit
#[derive(Clone, Debug)]
pub struct WasiThread {
    state: Arc<WasiThreadState>,
    layout: WasiMemoryLayout,
    start: ThreadStartType,

    // This is used for stack rewinds
    rewind: Option<RewindResult>,
}

impl WasiThread {
    pub fn id(&self) -> WasiThreadId {
        self.state.id
    }

    /// Sets that a rewind will take place
    pub(crate) fn set_rewind(&mut self, rewind: RewindResult) {
        self.rewind.replace(rewind);
    }

    /// Pops any rewinds that need to take place
    pub(crate) fn take_rewind(&mut self) -> Option<RewindResult> {
        self.rewind.take()
    }

    /// Gets the thread start type for this thread
    pub fn thread_start_type(&self) -> ThreadStartType {
        self.start
    }

    /// Returns true if a rewind of a particular type has been queued
    /// for processed by a rewind operation
    pub(crate) fn has_rewind_of_type(&self, type_: HandleRewindType) -> bool {
        match type_ {
            HandleRewindType::ResultDriven => match &self.rewind {
                Some(rewind) => match rewind.rewind_result {
                    RewindResultType::RewindRestart => true,
                    RewindResultType::RewindWithoutResult => false,
                    RewindResultType::RewindWithResult(_) => true,
                },
                None => false,
            },
            HandleRewindType::ResultLess => match &self.rewind {
                Some(rewind) => match rewind.rewind_result {
                    RewindResultType::RewindRestart => true,
                    RewindResultType::RewindWithoutResult => true,
                    RewindResultType::RewindWithResult(_) => false,
                },
                None => false,
            },
        }
    }

    /// Sets a flag that tells others if this thread is currently
    /// deep sleeping
    pub(crate) fn set_deep_sleeping(&self, val: bool) {
        self.state.deep_sleeping.store(val, Ordering::SeqCst);
    }

    /// Reads a flag that determines if this thread is currently
    /// deep sleeping
    pub(crate) fn is_deep_sleeping(&self) -> bool {
        self.state.deep_sleeping.load(Ordering::SeqCst)
    }

    /// Sets a flag that tells others that this thread is currently
    /// check pointing itself
    #[cfg(feature = "journal")]
    pub(crate) fn set_checkpointing(&self, val: bool) {
        self.state.check_pointing.store(val, Ordering::SeqCst);
    }

    /// Reads a flag that determines if this thread is currently
    /// check pointing itself or not
    #[cfg(feature = "journal")]
    pub(crate) fn is_check_pointing(&self) -> bool {
        self.state.check_pointing.load(Ordering::SeqCst)
    }

    /// Gets the memory layout for this thread
    #[allow(dead_code)]
    pub(crate) fn memory_layout(&self) -> &WasiMemoryLayout {
        &self.layout
    }

    /// Gets the memory layout for this thread
    pub(crate) fn set_memory_layout(&mut self, layout: WasiMemoryLayout) {
        self.layout = layout;
    }
}

/// A guard that ensures a thread is marked as terminated when dropped.
///
/// Normally the thread result should be manually registered with
/// [`WasiThread::set_status_running`] or [`WasiThread::set_status_finished`],
/// but this guard can ensure that the thread is marked as terminated even if
/// this is forgotten or a panic occurs.
pub struct WasiThreadRunGuard {
    pub thread: WasiThread,
}

impl WasiThreadRunGuard {
    pub fn new(thread: WasiThread) -> Self {
        Self { thread }
    }
}

impl Drop for WasiThreadRunGuard {
    fn drop(&mut self) {
        self.thread
            .set_status_finished(Err(
                crate::RuntimeError::new("Thread manager disconnected").into()
            ));
    }
}

/// Represents the memory layout of the parts that the thread itself uses
pub use wasmer_wasix_types::wasix::WasiMemoryLayout;

#[derive(Clone, Debug)]
pub enum RewindResultType {
    // The rewind must restart the operation it had already started
    RewindRestart,
    // The rewind has been triggered and should be handled but has not result
    RewindWithoutResult,
    // The rewind has been triggered and should be handled with the supplied result
    RewindWithResult(Bytes),
}

// Contains the result of a rewind operation
#[derive(Clone, Debug)]
pub(crate) struct RewindResult {
    /// Memory stack used to restore the memory stack (thing that holds local variables) back to where it was
    pub memory_stack: Option<Bytes>,
    /// Generic serialized object passed back to the rewind resumption code
    /// (uses the bincode serializer)
    pub rewind_result: RewindResultType,
}

#[derive(Debug)]
struct WasiThreadState {
    is_main: bool,
    pid: WasiProcessId,
    id: WasiThreadId,
    signals: Mutex<(Vec<Signal>, Vec<Waker>)>,
    stack: Mutex<ThreadStack>,
    status: Arc<OwnedTaskStatus>,
    #[cfg(feature = "journal")]
    check_pointing: AtomicBool,
    deep_sleeping: AtomicBool,

    // Registers the task termination with the ControlPlane on drop.
    // Never accessed, since it's a drop guard.
    _task_count_guard: TaskCountGuard,
}

static NO_MORE_BYTES: [u8; 0] = [0u8; 0];

impl WasiThread {
    pub fn new(
        pid: WasiProcessId,
        id: WasiThreadId,
        is_main: bool,
        status: Arc<OwnedTaskStatus>,
        guard: TaskCountGuard,
        layout: WasiMemoryLayout,
        start: ThreadStartType,
    ) -> Self {
        Self {
            state: Arc::new(WasiThreadState {
                is_main,
                pid,
                id,
                status,
                signals: Mutex::new((Vec::new(), Vec::new())),
                stack: Mutex::new(ThreadStack::default()),
                #[cfg(feature = "journal")]
                check_pointing: AtomicBool::new(false),
                deep_sleeping: AtomicBool::new(false),
                _task_count_guard: guard,
            }),
            layout,
            start,
            rewind: None,
        }
    }

    /// Returns the process ID
    pub fn pid(&self) -> WasiProcessId {
        self.state.pid
    }

    /// Returns the thread ID
    pub fn tid(&self) -> WasiThreadId {
        self.state.id
    }

    /// Returns true if this thread is the main thread
    pub fn is_main(&self) -> bool {
        self.state.is_main
    }

    /// Get a join handle to watch the task status.
    pub fn join_handle(&self) -> TaskJoinHandle {
        self.state.status.handle()
    }

    // TODO: this should be private, access should go through utility methods.
    pub fn signals(&self) -> &Mutex<(Vec<Signal>, Vec<Waker>)> {
        &self.state.signals
    }

    pub fn set_status_running(&self) {
        self.state.status.set_running();
    }

    /// Gets or sets the exit code based of a signal that was received
    /// Note: if the exit code was already set earlier this method will
    /// just return that earlier set exit code
    pub fn set_or_get_exit_code_for_signal(&self, sig: Signal) -> ExitCode {
        let default_exitcode: ExitCode = match sig {
            Signal::Sigquit | Signal::Sigabrt => Errno::Success.into(),
            Signal::Sigpipe => Errno::Pipe.into(),
            _ => Errno::Intr.into(),
        };
        // This will only set the status code if its not already set
        self.set_status_finished(Ok(default_exitcode));
        self.try_join()
            .map(|r| r.unwrap_or(default_exitcode))
            .unwrap_or(default_exitcode)
    }

    /// Marks the thread as finished (which will cause anyone that
    /// joined on it to wake up)
    pub fn set_status_finished(&self, res: Result<ExitCode, WasiRuntimeError>) {
        self.state.status.set_finished(res.map_err(Arc::new));
    }

    /// Waits until the thread is finished or the timeout is reached
    pub async fn join(&self) -> Result<ExitCode, Arc<WasiRuntimeError>> {
        self.state.status.await_termination().await
    }

    /// Attempts to join on the thread
    pub fn try_join(&self) -> Option<Result<ExitCode, Arc<WasiRuntimeError>>> {
        self.state.status.status().into_finished()
    }

    /// Adds a signal for this thread to process
    pub fn signal(&self, signal: Signal) {
        let tid = self.tid();
        tracing::trace!(%tid, "signal-thread({:?})", signal);

        let mut guard = self.state.signals.lock().unwrap();
        if !guard.0.contains(&signal) {
            guard.0.push(signal);
        }
        guard.1.drain(..).for_each(|w| w.wake());
    }

    /// Returns all the signals that are waiting to be processed
    pub fn has_signal(&self, signals: &[Signal]) -> bool {
        let guard = self.state.signals.lock().unwrap();
        for s in guard.0.iter() {
            if signals.contains(s) {
                return true;
            }
        }
        false
    }

    /// Waits for a signal to arrive
    pub async fn wait_for_signal(&self) {
        // This poller will process any signals when the main working function is idle
        struct SignalPoller<'a> {
            thread: &'a WasiThread,
        }
        impl<'a> std::future::Future for SignalPoller<'a> {
            type Output = ();
            fn poll(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Self::Output> {
                if self.thread.has_signals_or_subscribe(cx.waker()) {
                    return std::task::Poll::Ready(());
                }
                std::task::Poll::Pending
            }
        }
        SignalPoller { thread: self }.await
    }

    /// Returns all the signals that are waiting to be processed
    pub fn pop_signals_or_subscribe(&self, waker: &Waker) -> Option<Vec<Signal>> {
        let mut guard = self.state.signals.lock().unwrap();
        let mut ret = Vec::new();
        std::mem::swap(&mut ret, &mut guard.0);
        match ret.is_empty() {
            true => {
                if !guard.1.iter().any(|w| w.will_wake(waker)) {
                    guard.1.push(waker.clone());
                }
                None
            }
            false => Some(ret),
        }
    }

    /// Returns all the signals that are waiting to be processed
    pub fn has_signals_or_subscribe(&self, waker: &Waker) -> bool {
        let mut guard = self.state.signals.lock().unwrap();
        let has_signals = !guard.0.is_empty();
        if !has_signals && !guard.1.iter().any(|w| w.will_wake(waker)) {
            guard.1.push(waker.clone());
        }
        has_signals
    }

    /// Returns all the signals that are waiting to be processed
    pub fn pop_signals(&self) -> Vec<Signal> {
        let mut guard = self.state.signals.lock().unwrap();
        let mut ret = Vec::new();
        std::mem::swap(&mut ret, &mut guard.0);
        ret
    }

    /// Adds a stack snapshot and removes dead ones
    pub fn add_snapshot(
        &self,
        mut memory_stack: &[u8],
        mut memory_stack_corrected: &[u8],
        hash: u128,
        rewind_stack: &[u8],
        store_data: &[u8],
    ) {
        // Lock the stack
        let mut stack = self.state.stack.lock().unwrap();
        let mut pstack = stack.deref_mut();
        loop {
            // First we validate if the stack is no longer valid
            let memory_stack_before = pstack.memory_stack.len();
            let memory_stack_after = memory_stack.len();
            if memory_stack_before > memory_stack_after
                || (!pstack
                    .memory_stack
                    .iter()
                    .zip(memory_stack.iter())
                    .any(|(a, b)| *a == *b)
                    && !pstack
                        .memory_stack_corrected
                        .iter()
                        .zip(memory_stack.iter())
                        .any(|(a, b)| *a == *b))
            {
                // The stacks have changed so need to start again at this segment
                let mut new_stack = ThreadStack {
                    memory_stack: memory_stack.to_vec(),
                    memory_stack_corrected: memory_stack_corrected.to_vec(),
                    ..Default::default()
                };
                std::mem::swap(pstack, &mut new_stack);
                memory_stack = &NO_MORE_BYTES[..];
                memory_stack_corrected = &NO_MORE_BYTES[..];

                // Output debug info for the dead stack
                let mut disown = Some(Box::new(new_stack));
                if let Some(disown) = disown.as_ref() {
                    if !disown.snapshots.is_empty() {
                        tracing::trace!(
                            "wasi[{}]::stacks forgotten (memory_stack_before={}, memory_stack_after={})",
                            self.pid(),
                            memory_stack_before,
                            memory_stack_after
                        );
                    }
                }
                let mut total_forgotten = 0usize;
                while let Some(disowned) = disown {
                    for _hash in disowned.snapshots.keys() {
                        total_forgotten += 1;
                    }
                    disown = disowned.next;
                }
                if total_forgotten > 0 {
                    tracing::trace!(
                        "wasi[{}]::stack has been forgotten (cnt={})",
                        self.pid(),
                        total_forgotten
                    );
                }
            } else {
                memory_stack = &memory_stack[pstack.memory_stack.len()..];
                memory_stack_corrected =
                    &memory_stack_corrected[pstack.memory_stack_corrected.len()..];
            }

            // If there is no more memory stack then we are done and can add the call stack
            if memory_stack.is_empty() {
                break;
            }

            // Otherwise we need to add a next stack pointer and continue the iterations
            if pstack.next.is_none() {
                let new_stack = ThreadStack {
                    memory_stack: memory_stack.to_vec(),
                    memory_stack_corrected: memory_stack_corrected.to_vec(),
                    ..Default::default()
                };
                pstack.next.replace(Box::new(new_stack));
            }
            pstack = pstack.next.as_mut().unwrap();
        }

        // Add the call stack
        pstack.snapshots.insert(
            hash,
            ThreadSnapshot {
                call_stack: BytesMut::from(rewind_stack).freeze(),
                store_data: BytesMut::from(store_data).freeze(),
            },
        );
    }

    /// Gets a snapshot that was previously addedf
    pub fn get_snapshot(&self, hash: u128) -> Option<(BytesMut, Bytes, Bytes)> {
        let mut memory_stack = BytesMut::new();

        let stack = self.state.stack.lock().unwrap();
        let mut pstack = stack.deref();
        loop {
            memory_stack.extend(pstack.memory_stack_corrected.iter());
            if let Some(snapshot) = pstack.snapshots.get(&hash) {
                return Some((
                    memory_stack,
                    snapshot.call_stack.clone(),
                    snapshot.store_data.clone(),
                ));
            }
            if let Some(next) = pstack.next.as_ref() {
                pstack = next.deref();
            } else {
                return None;
            }
        }
    }

    // Copy the stacks from another thread
    pub fn copy_stack_from(&self, other: &WasiThread) {
        let mut stack = {
            let stack_guard = other.state.stack.lock().unwrap();
            stack_guard.clone()
        };

        let mut stack_guard = self.state.stack.lock().unwrap();
        std::mem::swap(stack_guard.deref_mut(), &mut stack);
    }
}

#[derive(Debug)]
pub struct WasiThreadHandleProtected {
    thread: WasiThread,
    inner: Weak<(Mutex<WasiProcessInner>, Condvar)>,
}

#[derive(Debug, Clone)]
pub struct WasiThreadHandle {
    protected: Arc<WasiThreadHandleProtected>,
}

impl WasiThreadHandle {
    pub(crate) fn new(
        thread: WasiThread,
        inner: &Arc<(Mutex<WasiProcessInner>, Condvar)>,
    ) -> WasiThreadHandle {
        Self {
            protected: Arc::new(WasiThreadHandleProtected {
                thread,
                inner: Arc::downgrade(inner),
            }),
        }
    }

    pub fn id(&self) -> WasiThreadId {
        self.protected.thread.tid()
    }

    pub fn as_thread(&self) -> WasiThread {
        self.protected.thread.clone()
    }
}

impl Drop for WasiThreadHandleProtected {
    fn drop(&mut self) {
        let id = self.thread.tid();
        if let Some(inner) = Weak::upgrade(&self.inner) {
            let mut inner = inner.0.lock().unwrap();
            if let Some(ctrl) = inner.threads.remove(&id) {
                ctrl.set_status_finished(Ok(Errno::Success.into()));
            }
            inner.thread_count -= 1;
        }
    }
}

impl std::ops::Deref for WasiThreadHandle {
    type Target = WasiThread;

    fn deref(&self) -> &Self::Target {
        &self.protected.thread
    }
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum WasiThreadError {
    #[error("Multithreading is not supported")]
    Unsupported,
    #[error("The method named is not an exported function")]
    MethodNotFound,
    #[error("Failed to create the requested memory - {0}")]
    MemoryCreateFailed(MemoryError),
    #[error("{0}")]
    ExportError(ExportError),
    #[error("Failed to create the instance")]
    // Note: Boxed so we can keep the error size down
    InstanceCreateFailed(Box<InstantiationError>),
    #[error("Initialization function failed - {0}")]
    InitFailed(Arc<anyhow::Error>),
    /// This will happen if WASM is running in a thread has not been created by the spawn_wasm call
    #[error("WASM context is invalid")]
    InvalidWasmContext,
}

impl From<WasiThreadError> for Errno {
    fn from(a: WasiThreadError) -> Errno {
        match a {
            WasiThreadError::Unsupported => Errno::Notsup,
            WasiThreadError::MethodNotFound => Errno::Inval,
            WasiThreadError::MemoryCreateFailed(_) => Errno::Nomem,
            WasiThreadError::ExportError(_) => Errno::Noexec,
            WasiThreadError::InstanceCreateFailed(_) => Errno::Noexec,
            WasiThreadError::InitFailed(_) => Errno::Noexec,
            WasiThreadError::InvalidWasmContext => Errno::Noexec,
        }
    }
}
