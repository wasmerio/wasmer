use std::{
    collections::HashMap,
    sync::{
        Arc, RwLock, Weak,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use crate::{WasiProcess, WasiProcessId, os::task::process::WasiProcessData};
use wasmer_types::ModuleHash;

#[derive(Debug, Clone)]
pub struct WasiControlPlane {
    state: Arc<State>,
}

#[derive(Debug, Clone)]
pub struct WasiControlPlaneHandle {
    inner: std::sync::Weak<State>,
}

impl WasiControlPlaneHandle {
    fn new(inner: &Arc<State>) -> Self {
        Self {
            inner: Arc::downgrade(inner),
        }
    }

    pub fn upgrade(&self) -> Option<WasiControlPlane> {
        self.inner.upgrade().map(|state| WasiControlPlane { state })
    }

    pub fn must_upgrade(&self) -> WasiControlPlane {
        let state = self.inner.upgrade().expect("control plane unavailable");
        WasiControlPlane { state }
    }
}

#[derive(Debug, Clone)]
pub struct ControlPlaneConfig {
    /// Total number of tasks (processes + threads) that can be spawned.
    pub max_task_count: Option<usize>,
    /// Flag that indicates if asynchronous threading is enables (opt-in)
    pub enable_asynchronous_threading: bool,
    /// Enables an exponential backoff of the process CPU usage when there
    /// are no active run tokens (when set holds the maximum amount of
    /// time that it will pause the CPU)
    /// (default = off)
    pub enable_exponential_cpu_backoff: Option<Duration>,
}

impl ControlPlaneConfig {
    pub fn new() -> Self {
        Self {
            max_task_count: None,
            enable_asynchronous_threading: false,
            enable_exponential_cpu_backoff: None,
        }
    }
}

impl Default for ControlPlaneConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
struct State {
    config: ControlPlaneConfig,

    /// Total number of active tasks (threads) across all processes.
    task_count: Arc<AtomicUsize>,

    /// Mutable state.
    mutable: RwLock<MutableState>,
}

#[derive(Debug)]
struct MutableState {
    /// Seed used to generate process ID's
    process_seed: u32,
    /// The processes running on this machine
    processes: HashMap<WasiProcessId, Weak<WasiProcessData>>,
    // TODO: keep a queue of terminated process ids for id reuse.
}

impl WasiControlPlane {
    pub fn new(config: ControlPlaneConfig) -> Self {
        Self {
            state: Arc::new(State {
                config,
                task_count: Arc::new(AtomicUsize::new(0)),
                mutable: RwLock::new(MutableState {
                    process_seed: 0,
                    processes: Default::default(),
                }),
            }),
        }
    }

    pub fn handle(&self) -> WasiControlPlaneHandle {
        WasiControlPlaneHandle::new(&self.state)
    }

    /// Get the current count of active tasks (threads).
    fn active_task_count(&self) -> usize {
        self.state.task_count.load(Ordering::SeqCst)
    }

    /// Returns the configuration for this control plane
    pub(crate) fn config(&self) -> &ControlPlaneConfig {
        &self.state.config
    }

    /// Register a new task.
    ///
    // Currently just increments the task counter.
    pub(crate) fn register_task(&self) -> Result<TaskCountGuard, ControlPlaneError> {
        let count = self.state.task_count.fetch_add(1, Ordering::SeqCst);
        if let Some(max) = self.state.config.max_task_count
            && count > max
        {
            self.state.task_count.fetch_sub(1, Ordering::SeqCst);
            return Err(ControlPlaneError::TaskLimitReached { max: count });
        }
        Ok(TaskCountGuard(self.state.task_count.clone()))
    }

    /// Creates a new process
    pub fn new_process(&self, module_hash: ModuleHash) -> Result<WasiProcess, ControlPlaneError> {
        if let Some(max) = self.state.config.max_task_count
            && self.active_task_count() >= max
        {
            // NOTE: task count is not incremented here, only when new threads are spawned.
            // A process will always have a main thread.
            return Err(ControlPlaneError::TaskLimitReached { max });
        }

        // The pid has to be known up front so it can be baked into both the process
        // and its `WasiProcessInner` at construction time.
        let pid = self.state.mutable.write().unwrap().next_process_id()?;

        let proc = WasiProcess::new(pid, module_hash, self.handle());

        // Only a `Weak` goes into the table, so the process is freed as soon as the
        // last real handle is dropped; `WasiProcessData::drop` then removes this entry.
        self.state
            .mutable
            .write()
            .unwrap()
            .processes
            .insert(pid, Arc::downgrade(&proc.0));

        Ok(proc)
    }

    /// Removes a process from the process table.
    ///
    /// Called from `WasiProcessData::drop`; not meant to be invoked directly, since
    /// removing a still-referenced process would make it unreachable via
    /// [`Self::get_process`] while it is still running.
    pub(crate) fn deregister_process(&self, pid: WasiProcessId) {
        if let Ok(mut mutable) = self.state.mutable.write() {
            mutable.processes.remove(&pid);
        }
    }

    /// Generates a new process ID
    pub fn generate_id(&self) -> Result<WasiProcessId, ControlPlaneError> {
        let mut mutable = self.state.mutable.write().unwrap();
        mutable.next_process_id()
    }

    /// Gets a reference to a running process
    pub fn get_process(&self, pid: WasiProcessId) -> Option<WasiProcess> {
        self.state
            .mutable
            .read()
            .unwrap()
            .processes
            .get(&pid)
            .and_then(Weak::upgrade)
            .map(WasiProcess)
    }

    /// Number of processes currently in the process table.
    ///
    /// Includes entries whose process has been dropped but whose `Drop` has not yet
    /// removed them, so this is only exact once all handles are released.
    pub fn process_count(&self) -> usize {
        self.state.mutable.read().unwrap().processes.len()
    }
}

impl MutableState {
    fn next_process_id(&mut self) -> Result<WasiProcessId, ControlPlaneError> {
        // TODO: reuse terminated ids, handle wrap-around, ...
        let id = self.process_seed.checked_add(1).ok_or({
            ControlPlaneError::TaskLimitReached {
                max: u32::MAX as usize,
            }
        })?;
        self.process_seed = id;
        Ok(WasiProcessId::from(id))
    }
}

impl Default for WasiControlPlane {
    fn default() -> Self {
        let config = ControlPlaneConfig::default();
        Self::new(config)
    }
}

/// Guard that ensures the [`WasiControlPlane`] task counter is decremented when dropped.
#[derive(Debug)]
pub struct TaskCountGuard(Arc<AtomicUsize>);

impl Drop for TaskCountGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(thiserror::Error, PartialEq, Eq, Clone, Debug)]
pub enum ControlPlaneError {
    /// The maximum number of execution tasks has been reached.
    #[error("The maximum number of execution tasks has been reached ({max})")]
    TaskLimitReached {
        /// The maximum number of tasks.
        max: usize,
    },
}

#[cfg(test)]
mod tests {
    use std::sync::{Condvar, Mutex};

    use wasmer_wasix_types::wasix::ThreadStartType;

    use crate::os::task::thread::WasiMemoryLayout;

    use super::*;

    /// Runs a process to completion, returning weak probes on `WasiProcessData` and on
    /// `WasiProcessInner` (which holds the linear memory). Both are checked because they
    /// are freed independently.
    #[allow(clippy::type_complexity)]
    fn spawn_and_finish(
        plane: &WasiControlPlane,
    ) -> (
        WasiProcessId,
        Weak<WasiProcessData>,
        Weak<(Mutex<crate::os::task::WasiProcessInner>, Condvar)>,
    ) {
        let proc = plane.new_process(ModuleHash::random()).unwrap();
        let pid = proc.pid();
        let data = Arc::downgrade(&proc.0);
        let inner = Arc::downgrade(&proc.inner);

        let handle = proc
            .new_thread(WasiMemoryLayout::default(), ThreadStartType::MainThread)
            .unwrap();
        drop(handle);
        drop(proc);

        (pid, data, inner)
    }

    /// A terminated process must leave the process table, and its memory must be freed.
    #[test]
    fn terminated_process_is_deregistered() {
        let plane = WasiControlPlane::default();
        let (pid, data, inner) = spawn_and_finish(&plane);

        assert!(
            data.upgrade().is_none(),
            "WasiProcessData is still alive after termination"
        );
        assert!(
            inner.upgrade().is_none(),
            "WasiProcessInner (and its linear memory) is still alive after termination"
        );
        assert!(
            plane.get_process(pid).is_none(),
            "terminated process is still reachable via get_process"
        );
        assert_eq!(
            plane.process_count(),
            0,
            "process table still holds an entry for the terminated process"
        );
    }

    /// Many short-lived processes (the `shell_exec` workload) must not accumulate.
    #[test]
    fn short_lived_processes_do_not_accumulate() {
        let plane = WasiControlPlane::default();

        for _ in 0..100 {
            let (_, data, inner) = spawn_and_finish(&plane);
            assert!(data.upgrade().is_none() && inner.upgrade().is_none());
        }

        assert_eq!(plane.process_count(), 0, "process table grew unboundedly");
    }

    /// De-registration is driven by the last handle dropping, not by the process
    /// exiting. This is what keeps `proc_join` working, since `join_children` resolves
    /// a child through `get_process` after it has exited.
    #[test]
    fn live_process_stays_registered() {
        let plane = WasiControlPlane::default();

        let proc = plane.new_process(ModuleHash::random()).unwrap();
        let pid = proc.pid();

        // Main thread runs and exits, but someone (e.g. the parent) still holds a handle.
        let handle = proc
            .new_thread(WasiMemoryLayout::default(), ThreadStartType::MainThread)
            .unwrap();
        drop(handle);

        assert!(
            plane.get_process(pid).is_some(),
            "an exited but still-referenced process must remain reachable so it can be joined"
        );
        assert_eq!(plane.process_count(), 1);

        drop(proc);
        assert!(plane.get_process(pid).is_none());
        assert_eq!(plane.process_count(), 0);
    }

    /// Simple test to ensure task limits are respected.
    #[test]
    fn test_control_plane_task_limits() {
        let p = WasiControlPlane::new(ControlPlaneConfig {
            max_task_count: Some(2),
            enable_asynchronous_threading: false,
            enable_exponential_cpu_backoff: None,
        });

        let p1 = p.new_process(ModuleHash::random()).unwrap();
        let _t1 = p1
            .new_thread(WasiMemoryLayout::default(), ThreadStartType::MainThread)
            .unwrap();
        let _t2 = p1
            .new_thread(WasiMemoryLayout::default(), ThreadStartType::MainThread)
            .unwrap();

        assert_eq!(
            p.new_process(ModuleHash::random()).unwrap_err(),
            ControlPlaneError::TaskLimitReached { max: 2 }
        );
    }

    /// Simple test to ensure task limits are respected and that thread drop guards work.
    #[test]
    fn test_control_plane_task_limits_with_dropped_threads() {
        let p = WasiControlPlane::new(ControlPlaneConfig {
            max_task_count: Some(2),
            enable_asynchronous_threading: false,
            enable_exponential_cpu_backoff: None,
        });

        let p1 = p.new_process(ModuleHash::random()).unwrap();

        for _ in 0..10 {
            let _thread = p1
                .new_thread(WasiMemoryLayout::default(), ThreadStartType::MainThread)
                .unwrap();
        }

        let _t1 = p1
            .new_thread(WasiMemoryLayout::default(), ThreadStartType::MainThread)
            .unwrap();
        let _t2 = p1
            .new_thread(WasiMemoryLayout::default(), ThreadStartType::MainThread)
            .unwrap();

        assert_eq!(
            p.new_process(ModuleHash::random()).unwrap_err(),
            ControlPlaneError::TaskLimitReached { max: 2 }
        );
    }
}
