use std::{
    collections::HashMap,
    convert::TryInto,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, RwLock, RwLockReadGuard, RwLockWriteGuard, Weak,
    },
    time::Duration,
};

use crate::WasiRuntimeError;
use tracing::trace;
use wasmer_wasix_types::{
    types::Signal,
    wasi::{Errno, ExitCode, Snapshot0Clockid, TlKey, TlUser, TlVal},
};

use crate::{
    os::task::signal::WasiSignalInterval, syscalls::platform_clock_time_get, WasiThread,
    WasiThreadHandle, WasiThreadId,
};

use super::{
    control_plane::{ControlPlaneError, WasiControlPlaneHandle},
    signal::{SignalDeliveryError, SignalHandlerAbi},
    task_join_handle::OwnedTaskStatus,
};

/// Represents the ID of a sub-process
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
        val.0 as u32
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
// TODO: fields should be private and only accessed via methods.
#[derive(Debug, Clone)]
pub struct WasiProcess {
    /// Unique ID of this process
    pub(crate) pid: WasiProcessId,
    /// List of all the children spawned from this thread
    pub(crate) parent: Option<Weak<RwLock<WasiProcessInner>>>,
    /// The inner protected region of the process
    pub(crate) inner: Arc<RwLock<WasiProcessInner>>,
    /// Reference back to the compute engine
    // TODO: remove this reference, access should happen via separate state instead
    // (we don't want cyclical references)
    pub(crate) compute: WasiControlPlaneHandle,
    /// Reference to the exit code for the main thread
    pub(crate) finished: Arc<OwnedTaskStatus>,
    /// Number of threads waiting for children to exit
    pub(crate) waiting: Arc<AtomicU32>,
}

// TODO: fields should be private and only accessed via methods.
#[derive(Debug)]
pub struct WasiProcessInner {
    /// Unique ID of this process
    pub pid: WasiProcessId,
    /// The threads that make up this process
    pub threads: HashMap<WasiThreadId, WasiThread>,
    /// Number of threads running for this process
    pub thread_count: u32,
    /// All the thread local variables
    pub thread_local: HashMap<(WasiThreadId, TlKey), TlVal>,
    /// User data associated with thread local data
    pub thread_local_user_data: HashMap<TlKey, TlUser>,
    /// Seed used to generate thread local keys
    pub thread_local_seed: TlKey,
    /// Signals that will be triggered at specific intervals
    pub signal_intervals: HashMap<Signal, WasiSignalInterval>,
    /// List of all the children spawned from this thread
    pub children: Vec<WasiProcess>,
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
    pub fn new(pid: WasiProcessId, plane: WasiControlPlaneHandle) -> Self {
        WasiProcess {
            pid,
            parent: None,
            compute: plane,
            inner: Arc::new(RwLock::new(WasiProcessInner {
                pid,
                threads: Default::default(),
                thread_count: Default::default(),
                thread_local: Default::default(),
                thread_local_user_data: Default::default(),
                thread_local_seed: Default::default(),
                signal_intervals: Default::default(),
                children: Default::default(),
            })),
            finished: Arc::new(OwnedTaskStatus::default()),
            waiting: Arc::new(AtomicU32::new(0)),
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

    /// Gains write access to the process internals
    // TODO: Make this private, all inner access should be exposed with methods.
    pub fn write(&self) -> RwLockWriteGuard<WasiProcessInner> {
        self.inner.write().unwrap()
    }

    /// Gains read access to the process internals
    // TODO: Make this private, all inner access should be exposed with methods.
    pub fn read(&self) -> RwLockReadGuard<WasiProcessInner> {
        self.inner.read().unwrap()
    }

    /// Creates a a thread and returns it
    pub fn new_thread(&self) -> Result<WasiThreadHandle, ControlPlaneError> {
        let control_plane = self.compute.must_upgrade();
        let task_count_guard = control_plane.register_task()?;

        // Determine if its the main thread or not
        let is_main = {
            let inner = self.inner.read().unwrap();
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
        let mut inner = self.inner.write().unwrap();
        let finished = if is_main {
            self.finished.clone()
        } else {
            Arc::new(OwnedTaskStatus::default())
        };

        // Insert the thread into the pool
        let ctrl = WasiThread::new(self.pid(), tid, is_main, finished, task_count_guard);
        inner.threads.insert(tid, ctrl.clone());
        inner.thread_count += 1;

        Ok(WasiThreadHandle::new(ctrl, &self.inner))
    }

    /// Gets a reference to a particular thread
    pub fn get_thread(&self, tid: &WasiThreadId) -> Option<WasiThread> {
        let inner = self.inner.read().unwrap();
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

        let inner = self.inner.read().unwrap();
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
        {
            let inner = self.inner.read().unwrap();
            if self.waiting.load(Ordering::Acquire) > 0 {
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
        let inner = self.inner.read().unwrap();
        for thread in inner.threads.values() {
            thread.signal(signal);
        }
    }

    /// Signals one of the threads every interval
    pub fn signal_interval(&self, signal: Signal, interval: Option<Duration>, repeat: bool) {
        let mut inner = self.inner.write().unwrap();

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
        let inner = self.inner.read().unwrap();
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

    /// Waits for all the children to be finished
    pub async fn join_children(&mut self) -> Option<Result<ExitCode, Arc<WasiRuntimeError>>> {
        let _guard = WasiProcessWait::new(self);
        let children: Vec<_> = {
            let inner = self.inner.read().unwrap();
            inner.children.clone()
        };
        if children.is_empty() {
            return None;
        }
        let mut waits = Vec::new();
        for child in children {
            if let Some(process) = self.compute.must_upgrade().get_process(child.pid) {
                let inner = self.inner.clone();
                waits.push(async move {
                    let join = process.join().await;
                    let mut inner = inner.write().unwrap();
                    inner.children.retain(|a| a.pid != child.pid);
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
    pub async fn join_any_child(&mut self) -> Result<Option<(WasiProcessId, ExitCode)>, Errno> {
        let _guard = WasiProcessWait::new(self);
        let children: Vec<_> = {
            let inner = self.inner.read().unwrap();
            inner.children.clone()
        };
        if children.is_empty() {
            return Err(Errno::Child);
        }

        let mut waits = Vec::new();
        for child in children {
            if let Some(process) = self.compute.must_upgrade().get_process(child.pid) {
                let inner = self.inner.clone();
                waits.push(async move {
                    let join = process.join().await;
                    let mut inner = inner.write().unwrap();
                    inner.children.retain(|a| a.pid != child.pid);
                    (child, join)
                })
            }
        }
        let (child, res) = futures::future::select_all(waits.into_iter().map(|a| Box::pin(a)))
            .await
            .0;

        let code =
            res.unwrap_or_else(|e| e.as_exit_code().unwrap_or_else(|| Errno::Canceled.into()));

        Ok(Some((child.pid, code)))
    }

    /// Terminate the process and all its threads
    pub fn terminate(&self, exit_code: ExitCode) {
        // FIXME: this is wrong, threads might still be running!
        // Need special logic for the main thread.
        let guard = self.inner.read().unwrap();
        for thread in guard.threads.values() {
            thread.set_status_finished(Ok(exit_code))
        }
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
