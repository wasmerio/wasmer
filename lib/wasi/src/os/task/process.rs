use std::{
    borrow::Cow,
    collections::HashMap,
    convert::TryInto,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard,
    },
    time::Duration,
};

use tracing::log::trace;
use wasmer_vbus::{BusSpawnedProcess, SignalHandlerAbi};
use wasmer_wasi_types::{
    types::Signal,
    wasi::{Errno, ExitCode, Snapshot0Clockid, TlKey, TlUser, TlVal},
};

use crate::{
    os::task::{control_plane::WasiControlPlane, signal::WasiSignalInterval, thread::ThreadStack},
    syscalls::platform_clock_time_get,
    WasiThread, WasiThreadHandle, WasiThreadId,
};

/// Represents the ID of a sub-process
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

impl Into<i32> for WasiProcessId {
    fn into(self) -> i32 {
        self.0 as i32
    }
}

impl From<u32> for WasiProcessId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}

impl Into<u32> for WasiProcessId {
    fn into(self) -> u32 {
        self.0 as u32
    }
}

impl std::fmt::Display for WasiProcessId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug)]
pub struct WasiProcessInner {
    /// The threads that make up this process
    pub threads: HashMap<WasiThreadId, WasiThread>,
    /// Number of threads running for this process
    pub thread_count: u32,
    /// Seed used to generate thread ID's
    pub thread_seed: WasiThreadId,
    /// All the thread local variables
    pub thread_local: HashMap<(WasiThreadId, TlKey), TlVal>,
    /// User data associated with thread local data
    pub thread_local_user_data: HashMap<TlKey, TlUser>,
    /// Seed used to generate thread local keys
    pub thread_local_seed: TlKey,
    /// Signals that will be triggered at specific intervals
    pub signal_intervals: HashMap<Signal, WasiSignalInterval>,
    /// Represents all the process spun up as a bus process
    pub bus_processes: HashMap<WasiProcessId, Box<BusSpawnedProcess>>,
    /// Indicates if the bus process can be reused
    pub bus_process_reuse: HashMap<Cow<'static, str>, WasiProcessId>,
}

/// Represents a process running within the compute state
#[derive(Debug, Clone)]
pub struct WasiProcess {
    /// Unique ID of this process
    pub(crate) pid: WasiProcessId,
    /// ID of the parent process
    pub(crate) ppid: WasiProcessId,
    /// The inner protected region of the process
    pub(crate) inner: Arc<RwLock<WasiProcessInner>>,
    /// Reference back to the compute engine
    pub(crate) compute: WasiControlPlane,
    /// Reference to the exit code for the main thread
    pub(crate) finished: Arc<Mutex<(Option<ExitCode>, tokio::sync::broadcast::Sender<()>)>>,
    /// List of all the children spawned from this thread
    pub(crate) children: Arc<RwLock<Vec<WasiProcessId>>>,
    /// Number of threads waiting for children to exit
    pub(crate) waiting: Arc<AtomicU32>,
}

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
    /// Gets the process ID of this process
    pub fn pid(&self) -> WasiProcessId {
        self.pid
    }

    /// Gets the process ID of the parent process
    pub fn ppid(&self) -> WasiProcessId {
        self.ppid
    }

    /// Gains write access to the process internals
    pub fn write(&self) -> RwLockWriteGuard<WasiProcessInner> {
        self.inner.write().unwrap()
    }

    /// Gains read access to the process internals
    pub fn read(&self) -> RwLockReadGuard<WasiProcessInner> {
        self.inner.read().unwrap()
    }

    /// Creates a a thread and returns it
    pub fn new_thread(&self) -> WasiThreadHandle {
        let mut inner = self.inner.write().unwrap();
        let id = inner.thread_seed.inc();

        let mut is_main = false;
        let finished = if inner.thread_count <= 0 {
            is_main = true;
            self.finished.clone()
        } else {
            Arc::new(Mutex::new((None, tokio::sync::broadcast::channel(1).0)))
        };

        let ctrl = WasiThread {
            pid: self.pid(),
            id,
            is_main,
            finished,
            signals: Arc::new(Mutex::new((
                Vec::new(),
                tokio::sync::broadcast::channel(1).0,
            ))),
            stack: Arc::new(Mutex::new(ThreadStack::default())),
        };
        inner.threads.insert(id, ctrl.clone());
        inner.thread_count += 1;

        WasiThreadHandle {
            id: Arc::new(id),
            thread: ctrl,
            inner: self.inner.clone(),
        }
    }

    /// Gets a reference to a particular thread
    pub fn get_thread(&self, tid: &WasiThreadId) -> Option<WasiThread> {
        let inner = self.inner.read().unwrap();
        inner.threads.get(tid).map(|a| a.clone())
    }

    /// Signals a particular thread in the process
    pub fn signal_thread(&self, tid: &WasiThreadId, signal: Signal) {
        let inner = self.inner.read().unwrap();
        if let Some(thread) = inner.threads.get(tid) {
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
        if self.waiting.load(Ordering::Acquire) > 0 {
            let children = self.children.read().unwrap();
            for pid in children.iter() {
                if let Some(process) = self.compute.get_process(*pid) {
                    process.signal_process(signal);
                }
            }
            return;
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

    /// Waits until the process is finished or the timeout is reached
    pub async fn join(&self) -> Option<ExitCode> {
        let _guard = WasiProcessWait::new(self);
        loop {
            let mut rx = {
                let finished = self.finished.lock().unwrap();
                if finished.0.is_some() {
                    return finished.0.clone();
                }
                finished.1.subscribe()
            };
            if rx.recv().await.is_err() {
                return None;
            }
        }
    }

    /// Attempts to join on the process
    pub fn try_join(&self) -> Option<ExitCode> {
        let guard = self.finished.lock().unwrap();
        guard.0.clone()
    }

    /// Waits for all the children to be finished
    pub async fn join_children(&mut self) -> Option<ExitCode> {
        let _guard = WasiProcessWait::new(self);
        let children: Vec<_> = {
            let children = self.children.read().unwrap();
            children.clone()
        };
        if children.is_empty() {
            return None;
        }
        let mut waits = Vec::new();
        for pid in children {
            if let Some(process) = self.compute.get_process(pid) {
                let children = self.children.clone();
                waits.push(async move {
                    let join = process.join().await;
                    let mut children = children.write().unwrap();
                    children.retain(|a| *a != pid);
                    join
                })
            }
        }
        futures::future::join_all(waits.into_iter())
            .await
            .into_iter()
            .filter_map(|a| a)
            .next()
    }

    /// Waits for any of the children to finished
    pub async fn join_any_child(&mut self) -> Result<Option<(WasiProcessId, ExitCode)>, Errno> {
        let _guard = WasiProcessWait::new(self);
        loop {
            let children: Vec<_> = {
                let children = self.children.read().unwrap();
                children.clone()
            };
            if children.is_empty() {
                return Err(Errno::Child);
            }

            let mut waits = Vec::new();
            for pid in children {
                if let Some(process) = self.compute.get_process(pid) {
                    let children = self.children.clone();
                    waits.push(async move {
                        let join = process.join().await;
                        let mut children = children.write().unwrap();
                        children.retain(|a| *a != pid);
                        join.map(|exit_code| (pid, exit_code))
                    })
                }
            }
            let woke = futures::future::select_all(waits.into_iter().map(|a| Box::pin(a)))
                .await
                .0;
            if let Some((pid, exit_code)) = woke {
                return Ok(Some((pid, exit_code)));
            }
        }
    }

    /// Terminate the process and all its threads
    pub fn terminate(&self, exit_code: ExitCode) {
        let guard = self.inner.read().unwrap();
        for thread in guard.threads.values() {
            thread.terminate(exit_code)
        }
    }

    /// Gains access to the compute control plane
    pub fn control_plane(&self) -> &WasiControlPlane {
        &self.compute
    }
}

impl SignalHandlerAbi for WasiProcess {
    fn signal(&self, sig: u8) {
        if let Ok(sig) = sig.try_into() {
            self.signal_process(sig);
        }
    }
}
