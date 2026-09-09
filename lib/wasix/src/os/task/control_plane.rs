use std::{
    collections::HashMap,
    sync::{
        Arc, RwLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use crate::{WasiProcess, WasiProcessId};
use wasmer_types::ModuleHash;
use wasmer_wasix_types::wasi::ExitCode;

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
    processes: HashMap<WasiProcessId, WasiProcess>,
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
    // FIXME: De-register terminated processes!
    // Currently they just accumulate.
    pub fn new_process(&self, module_hash: ModuleHash) -> Result<WasiProcess, ControlPlaneError> {
        self.new_process_with_parent(module_hash, None)
    }

    pub(super) fn new_process_with_parent(
        &self,
        module_hash: ModuleHash,
        parent: Option<&WasiProcess>,
    ) -> Result<WasiProcess, ControlPlaneError> {
        if let Some(max) = self.state.config.max_task_count
            && self.active_task_count() >= max
        {
            // NOTE: task count is not incremented here, only when new threads are spawned.
            // A process will always have a main thread.
            return Err(ControlPlaneError::TaskLimitReached { max });
        }

        // Create the process first to do all the allocations before locking.
        let mut proc = WasiProcess::new(WasiProcessId::from(0), module_hash, self.handle());

        let mut mutable = self.state.mutable.write().unwrap();

        // Child creation and subtree shutdown take the same control-plane lock.
        // A child is either included in the shutdown snapshot or rejected here.
        let mut parent_inner = parent.map(WasiProcess::lock);
        if parent_inner
            .as_ref()
            .is_some_and(|inner| inner.forced_exit_code.is_some())
        {
            return Err(ControlPlaneError::ProcessTerminated);
        }

        let pid = mutable.next_process_id()?;
        proc.set_pid(pid);
        proc.parent = parent.map(|parent| Arc::downgrade(&parent.inner));
        mutable.processes.insert(pid, proc.clone());
        if let Some(parent_inner) = parent_inner.as_mut() {
            parent_inner.children.push(proc.clone());
        }
        Ok(proc)
    }

    pub(super) fn force_terminate(&self, root: &WasiProcess, exit_code: ExitCode) {
        let processes = {
            // Keep ancestry in the existing process registry, not in the reap
            // lists: proc_join can remove a child while it is still running.
            // Exclusivity also serializes competing force requests, so the
            // first requested exit code is latched throughout the whole family.
            #[allow(clippy::readonly_write_lock)]
            let mutable = self.state.mutable.write().unwrap();
            let mut children = HashMap::<_, Vec<_>>::new();
            for process in mutable.processes.values() {
                if let Some(parent) = &process.parent {
                    children
                        .entry(parent.as_ptr())
                        .or_default()
                        .push(process.clone());
                }
            }

            let mut processes = vec![root.clone()];
            let mut cursor = 0;
            while cursor < processes.len() {
                let process = &processes[cursor];
                process.lock().forced_exit_code.get_or_insert(exit_code);
                if let Some(descendants) = children.remove(&Arc::as_ptr(&process.inner)) {
                    processes.extend(descendants);
                }
                cursor += 1;
            }
            processes
        };

        // Complete children before parents and never wake user tasks under the
        // control-plane lock. All registration gates are already closed.
        for process in processes.into_iter().rev() {
            process.force_terminate_local(exit_code);
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
            .cloned()
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
    /// The owning control plane was dropped.
    #[error("The control plane is unavailable")]
    Unavailable,
    /// Forced execution-family shutdown must be requested on its root process.
    #[error("Forced termination requires a root process")]
    NotRootProcess,
    /// The process has been forcibly terminated and cannot create more work.
    #[error("The process has been forcibly terminated")]
    ProcessTerminated,
    /// The maximum number of execution tasks has been reached.
    #[error("The maximum number of execution tasks has been reached ({max})")]
    TaskLimitReached {
        /// The maximum number of tasks.
        max: usize,
    },
}

#[cfg(test)]
mod tests {
    use wasmer_wasix_types::wasix::ThreadStartType;

    use crate::os::task::thread::WasiMemoryLayout;

    use super::*;

    #[cfg(all(feature = "sys-thread", not(target_arch = "wasm32")))]
    #[tokio::test]
    async fn failed_fork_main_thread_does_not_leave_a_pending_child() {
        let plane = WasiControlPlane::new(ControlPlaneConfig {
            max_task_count: Some(4),
            ..ControlPlaneConfig::default()
        });
        let mut init = crate::WasiEnv::builder("fork-capacity-race")
            .engine(wasmer::Store::default().engine().clone())
            .build_init()
            .unwrap();
        init.control_plane = plane.clone();
        let env = crate::WasiEnv::from_init(init, ModuleHash::random()).unwrap();
        let parent = env.process.clone();

        // Pause child registration after its initial capacity check but before
        // it can return from new_child and create the child's main thread.
        let parent_guard = parent.lock();
        let fork = std::thread::spawn(move || env.fork());
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while plane.state.mutable.try_write().is_ok() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        let capacity_guards = (0..4)
            .map(|_| plane.register_task().unwrap())
            .collect::<Vec<_>>();
        drop(parent_guard);
        assert!(matches!(
            fork.join().unwrap(),
            Err(ControlPlaneError::TaskLimitReached { .. })
        ));
        assert!(parent.lock().children.is_empty());
        let children = plane
            .state
            .mutable
            .read()
            .unwrap()
            .processes
            .values()
            .filter(|process| process.parent.is_some())
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(children.len(), 1);
        assert!(children[0].try_join().is_some());
        drop(capacity_guards);
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
