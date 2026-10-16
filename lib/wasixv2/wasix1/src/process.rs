//! Per-process compat state: what v1 keeps about a process that Linux does not, keyed by pid.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

use wasixv2_syscalls::{Fd, TaskId};

use crate::calls;

/// The compat state of one process image. A new image (`execve`) gets a new one; threads share
/// it through the pid.
#[derive(Default)]
pub struct ProcState {
    /// Loaded on the process's first v1 call; see [`calls::startup::load`].
    pub(crate) startup: OnceLock<Arc<Startup>>,
    pub fs: calls::fs::State,
    pub net: calls::sock::State,
    pub proc: calls::proc::State,
}

impl ProcState {
    /// The state of a child that `fork` or `vfork` makes of this process: it has the same
    /// descriptors, so it announces the same preopens, and the same arguments. Families whose
    /// state follows descriptors or dispositions copy theirs here.
    pub fn fork(&self) -> Self {
        let startup = OnceLock::new();
        if let Some(loaded) = self.startup.get() {
            let _ = startup.set(loaded.clone());
        }
        Self {
            startup,
            fs: Default::default(),
            net: Default::default(),
            proc: self.proc.fork(),
        }
    }
}

/// What v1 reads once per image: the argument and environment strings, and the preopened
/// directories.
#[derive(Debug, Default)]
pub struct Startup {
    pub args: Vec<Vec<u8>>,
    pub env: Vec<Vec<u8>>,
    pub preopens: Vec<Preopen>,
}

/// A directory v1 announces through `fd_prestat_get`.
#[derive(Debug, Clone)]
pub struct Preopen {
    pub fd: Fd,
    /// The name `fd_prestat_dir_name` reports, such as `/`.
    pub name: Vec<u8>,
}

impl Startup {
    pub fn preopen(&self, fd: Fd) -> Option<&Preopen> {
        self.preopens.iter().find(|preopen| preopen.fd == fd)
    }
}

/// The compat state of every process that runs v1 code, by pid.
#[derive(Default)]
pub struct Procs {
    procs: Mutex<HashMap<TaskId, Arc<ProcState>>>,
}

impl Procs {
    /// The state of `pid`, created empty if the process has none yet (a vfork child before its
    /// parent's state was copied, a process whose image another ABI loaded).
    pub fn get_or_create(&self, pid: TaskId) -> Arc<ProcState> {
        self.procs.lock().unwrap().entry(pid).or_default().clone()
    }

    /// Starts `pid` over with `state`, as a new process image does.
    pub fn replace(&self, pid: TaskId, state: Arc<ProcState>) {
        self.procs.lock().unwrap().insert(pid, state);
    }

    /// Drops `pid`'s state if it still is `state`, when the image that made it ends.
    pub fn remove(&self, pid: TaskId, state: &Arc<ProcState>) {
        let mut procs = self.procs.lock().unwrap();
        if procs
            .get(&pid)
            .is_some_and(|current| Arc::ptr_eq(current, state))
        {
            procs.remove(&pid);
        }
    }
}
