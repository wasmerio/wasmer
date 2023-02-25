use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, RwLock,
    },
};

use crate::{WasiProcess, WasiProcessId};

#[derive(Debug, Clone)]
pub struct WasiControlPlane {
    state: Arc<State>,
}

#[derive(Debug, Clone)]
pub struct ControlPlaneConfig {
    /// Total number of tasks (processes + threads) that can be spawned.
    pub max_task_count: Option<usize>,
}

impl ControlPlaneConfig {
    pub fn new() -> Self {
        Self {
            max_task_count: None,
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

    /// Get the current count of active tasks (threads).
    fn active_task_count(&self) -> usize {
        self.state.task_count.load(Ordering::SeqCst)
    }

    /// Register a new task.
    ///
    // Currently just increments the task counter.
    pub(super) fn register_task(&self) -> Result<TaskCountGuard, ControlPlaneError> {
        let count = self.state.task_count.fetch_add(1, Ordering::SeqCst);
        if let Some(max) = self.state.config.max_task_count {
            if count > max {
                self.state.task_count.fetch_sub(1, Ordering::SeqCst);
                return Err(ControlPlaneError::TaskLimitReached { max: count });
            }
        }
        Ok(TaskCountGuard(self.state.task_count.clone()))
    }

    /// Creates a new process
    // FIXME: De-register terminated processes!
    // Currently they just accumulate.
    pub fn new_process(&self) -> Result<WasiProcess, ControlPlaneError> {
        if let Some(max) = self.state.config.max_task_count {
            if self.active_task_count() >= max {
                // NOTE: task count is not incremented here, only when new threads are spawned.
                // A process will always have a main thread.
                return Err(ControlPlaneError::TaskLimitReached { max });
            }
        }

        // Create the process first to do all the allocations before locking.
        let mut proc = WasiProcess::new(WasiProcessId::from(0), self.clone());

        let mut mutable = self.state.mutable.write().unwrap();

        let pid = mutable.next_process_id()?;
        proc.set_pid(pid);
        mutable.processes.insert(pid, proc.clone());
        Ok(proc)
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
    /// The maximum number of execution tasks has been reached.
    #[error("The maximum number of execution tasks has been reached ({max})")]
    TaskLimitReached {
        /// The maximum number of tasks.
        max: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simple test to ensure task limits are respected.
    #[test]
    fn test_control_plane_task_limits() {
        let p = WasiControlPlane::new(ControlPlaneConfig {
            max_task_count: Some(2),
        });

        let p1 = p.new_process().unwrap();
        let _t1 = p1.new_thread().unwrap();
        let _t2 = p1.new_thread().unwrap();

        assert_eq!(
            p.new_process().unwrap_err(),
            ControlPlaneError::TaskLimitReached { max: 2 }
        );
    }

    /// Simple test to ensure task limits are respected and that thread drop guards work.
    #[test]
    fn test_control_plane_task_limits_with_dropped_threads() {
        let p = WasiControlPlane::new(ControlPlaneConfig {
            max_task_count: Some(2),
        });

        let p1 = p.new_process().unwrap();

        for _ in 0..10 {
            let _thread = p1.new_thread().unwrap();
        }

        let _t1 = p1.new_thread().unwrap();
        let _t2 = p1.new_thread().unwrap();

        assert_eq!(
            p.new_process().unwrap_err(),
            ControlPlaneError::TaskLimitReached { max: 2 }
        );
    }
}
