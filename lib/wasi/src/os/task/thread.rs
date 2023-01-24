use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, RwLock},
};

use bytes::{Bytes, BytesMut};
use wasmer_wasi_types::{
    types::Signal,
    wasi::{Errno, ExitCode},
};

use crate::os::task::process::{WasiProcessId, WasiProcessInner};

use super::{control_plane::TaskCountGuard, task_join_handle::TaskJoinHandle};

/// Represents the ID of a WASI thread
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WasiThreadId(u32);

impl WasiThreadId {
    pub fn raw(&self) -> u32 {
        self.0
    }

    pub fn inc(&mut self) -> WasiThreadId {
        let ret = self.clone();
        self.0 += 1;
        ret
    }
}

impl From<i32> for WasiThreadId {
    fn from(id: i32) -> Self {
        Self(id as u32)
    }
}

impl Into<i32> for WasiThreadId {
    fn into(self) -> i32 {
        self.0 as i32
    }
}

impl From<u32> for WasiThreadId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}

impl From<WasiThreadId> for u32 {
    fn from(t: WasiThreadId) -> u32 {
        t.0 as u32
    }
}

impl std::fmt::Display for WasiThreadId {
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
}

#[derive(Debug)]
struct WasiThreadState {
    is_main: bool,
    pid: WasiProcessId,
    id: WasiThreadId,
    signals: Mutex<(Vec<Signal>, tokio::sync::broadcast::Sender<()>)>,
    stack: Mutex<ThreadStack>,
    finished: Arc<TaskJoinHandle>,

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
        finished: Arc<TaskJoinHandle>,
        guard: TaskCountGuard,
    ) -> Self {
        Self {
            state: Arc::new(WasiThreadState {
                is_main,
                pid,
                id,
                finished,
                signals: Mutex::new((Vec::new(), tokio::sync::broadcast::channel(1).0)),
                stack: Mutex::new(ThreadStack::default()),
                _task_count_guard: guard,
            }),
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

    // TODO: this should be private, access should go through utility methods.
    pub fn signals(&self) -> &Mutex<(Vec<Signal>, tokio::sync::broadcast::Sender<()>)> {
        &self.state.signals
    }

    /// Marks the thread as finished (which will cause anyone that
    /// joined on it to wake up)
    pub fn terminate(&self, exit_code: u32) {
        self.state.finished.terminate(exit_code);
    }

    /// Waits until the thread is finished or the timeout is reached
    pub async fn join(&self) -> Option<ExitCode> {
        self.state.finished.await_termination().await
    }

    /// Attempts to join on the thread
    pub fn try_join(&self) -> Option<ExitCode> {
        self.state.finished.get_exit_code()
    }

    /// Adds a signal for this thread to process
    pub fn signal(&self, signal: Signal) {
        let mut guard = self.state.signals.lock().unwrap();
        if guard.0.contains(&signal) == false {
            guard.0.push(signal);
        }
        let _ = guard.1.send(());
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

    /// Returns all the signals that are waiting to be processed
    pub fn pop_signals_or_subscribe(
        &self,
    ) -> Result<Vec<Signal>, tokio::sync::broadcast::Receiver<()>> {
        let mut guard = self.state.signals.lock().unwrap();
        let mut ret = Vec::new();
        std::mem::swap(&mut ret, &mut guard.0);
        match ret.is_empty() {
            true => Err(guard.1.subscribe()),
            false => Ok(ret),
        }
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
                || (pstack
                    .memory_stack
                    .iter()
                    .zip(memory_stack.iter())
                    .any(|(a, b)| *a == *b)
                    == false
                    && pstack
                        .memory_stack_corrected
                        .iter()
                        .zip(memory_stack.iter())
                        .any(|(a, b)| *a == *b)
                        == false)
            {
                // The stacks have changed so need to start again at this segment
                let mut new_stack = ThreadStack::default();
                new_stack.memory_stack = memory_stack.to_vec();
                new_stack.memory_stack_corrected = memory_stack_corrected.to_vec();
                std::mem::swap(pstack, &mut new_stack);
                memory_stack = &NO_MORE_BYTES[..];
                memory_stack_corrected = &NO_MORE_BYTES[..];

                // Output debug info for the dead stack
                let mut disown = Some(Box::new(new_stack));
                if let Some(disown) = disown.as_ref() {
                    if disown.snapshots.is_empty() == false {
                        tracing::trace!(
                            "wasi[{}]::stacks forgotten (memory_stack_before={}, memory_stack_after={})",
                            self.pid(),
                            memory_stack_before,
                            memory_stack_after
                        );
                    }
                }
                while let Some(disowned) = disown {
                    for hash in disowned.snapshots.keys() {
                        tracing::trace!(
                            "wasi[{}]::stack has been forgotten (hash={})",
                            self.pid(),
                            hash
                        );
                    }
                    disown = disowned.next;
                }
            } else {
                memory_stack = &memory_stack[pstack.memory_stack.len()..];
                memory_stack_corrected =
                    &memory_stack_corrected[pstack.memory_stack_corrected.len()..];
            }

            // If there is no more memory stack then we are done and can add the call stack
            if memory_stack.len() <= 0 {
                break;
            }

            // Otherwise we need to add a next stack pointer and continue the iterations
            if pstack.next.is_none() {
                let mut new_stack = ThreadStack::default();
                new_stack.memory_stack = memory_stack.to_vec();
                new_stack.memory_stack_corrected = memory_stack_corrected.to_vec();
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

#[derive(Debug, Clone)]
pub struct WasiThreadHandle {
    pub(super) id: Arc<WasiThreadId>,
    pub(super) thread: WasiThread,
    pub(super) inner: Arc<RwLock<WasiProcessInner>>,
}

impl WasiThreadHandle {
    pub fn id(&self) -> WasiThreadId {
        self.id.0.into()
    }

    pub fn as_thread(&self) -> WasiThread {
        self.thread.clone()
    }
}

impl Drop for WasiThreadHandle {
    fn drop(&mut self) {
        // We do this so we track when the last handle goes out of scope
        if let Some(id) = Arc::get_mut(&mut self.id) {
            let mut inner = self.inner.write().unwrap();
            if let Some(ctrl) = inner.threads.remove(id) {
                ctrl.terminate(0);
            }
            inner.thread_count -= 1;
        }
    }
}

impl std::ops::Deref for WasiThreadHandle {
    type Target = WasiThread;

    fn deref(&self) -> &Self::Target {
        &self.thread
    }
}

impl std::ops::DerefMut for WasiThreadHandle {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.thread
    }
}

#[derive(thiserror::Error, Debug)]
pub enum WasiThreadError {
    #[error("Multithreading is not supported")]
    Unsupported,
    #[error("The method named is not an exported function")]
    MethodNotFound,
    #[error("Failed to create the requested memory")]
    MemoryCreateFailed,
    /// This will happen if WASM is running in a thread has not been created by the spawn_wasm call
    #[error("WASM context is invalid")]
    InvalidWasmContext,
}

impl From<WasiThreadError> for Errno {
    fn from(a: WasiThreadError) -> Errno {
        match a {
            WasiThreadError::Unsupported => Errno::Notsup,
            WasiThreadError::MethodNotFound => Errno::Inval,
            WasiThreadError::MemoryCreateFailed => Errno::Fault,
            WasiThreadError::InvalidWasmContext => Errno::Noexec,
        }
    }
}
