use crate::os::task::process::WasiProcessInner;
use crate::{WasiProcess, WasiProcessId};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

#[derive(Debug, Clone)]
pub struct WasiControlPlane {
    /// The processes running on this machine
    pub(crate) processes: Arc<RwLock<HashMap<WasiProcessId, WasiProcess>>>,
    /// Seed used to generate process ID's
    pub(crate) process_seed: Arc<AtomicU32>,
    /// Allows for a PID to be reserved
    pub(crate) reserved: Arc<Mutex<HashSet<WasiProcessId>>>,
}

impl Default for WasiControlPlane {
    fn default() -> Self {
        Self {
            processes: Default::default(),
            process_seed: Arc::new(AtomicU32::new(0)),
            reserved: Default::default(),
        }
    }
}

impl WasiControlPlane {
    /// Reserves a PID and returns it
    pub fn reserve_pid(&self) -> WasiProcessId {
        let mut pid: WasiProcessId;
        loop {
            pid = self.process_seed.fetch_add(1, Ordering::AcqRel).into();

            {
                let mut guard = self.reserved.lock().unwrap();
                if guard.contains(&pid) {
                    continue;
                }
                guard.insert(pid);
            }

            {
                let guard = self.processes.read().unwrap();
                if guard.contains_key(&pid) == false {
                    break;
                }
            }

            {
                let mut guard = self.reserved.lock().unwrap();
                guard.remove(&pid);
            }
        }
        pid
    }

    /// Creates a new process
    pub fn new_process(&self) -> WasiProcess {
        let pid = self.reserve_pid();
        let ret = WasiProcess {
            pid,
            ppid: 0u32.into(),
            compute: self.clone(),
            inner: Arc::new(RwLock::new(WasiProcessInner {
                threads: Default::default(),
                thread_count: Default::default(),
                thread_seed: Default::default(),
                thread_local: Default::default(),
                thread_local_user_data: Default::default(),
                thread_local_seed: Default::default(),
                signal_intervals: Default::default(),
                bus_processes: Default::default(),
                bus_process_reuse: Default::default(),
            })),
            children: Arc::new(RwLock::new(Default::default())),
            finished: Arc::new(Mutex::new((None, tokio::sync::broadcast::channel(1).0))),
            waiting: Arc::new(AtomicU32::new(0)),
        };
        {
            let mut guard = self.processes.write().unwrap();
            guard.insert(pid, ret.clone());
        }
        {
            let mut guard = self.reserved.lock().unwrap();
            guard.remove(&pid);
        }
        ret
    }

    /// Gets a reference to a running process
    pub fn get_process(&self, pid: WasiProcessId) -> Option<WasiProcess> {
        let guard = self.processes.read().unwrap();
        guard.get(&pid).map(|a| a.clone())
    }
}
