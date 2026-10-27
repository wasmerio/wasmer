//! The per-instance half of the v1 ABI: entry points, signal delivery and vfork resumption as
//! wasix-libc expects them.

use std::{
    any::Any,
    collections::HashMap,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use wasixv2_exec::{
    AbiInstance, Entry, EntryReturn, ExecEnv, Role, Sigreturn, VforkResume, asyncify::StackId,
    call_guest,
};
use wasixv2_syscalls::{LocalFuture, SignalFrame, TaskId};
use wasmer::{AsyncFunctionEnvMut, Exports, Function, Instance, RuntimeError, Value};

use crate::process::{Commands, ProcState, Procs};

/// `_initialize`, which reactors and some commands export; it runs before `_start`.
const INITIALIZE: &str = "_initialize";
const START: &str = "_start";
/// The thread entry, as `wasi_thread_start(tid, start_arg)`.
pub const THREAD_ENTRY: &str = "wasi_thread_start";
/// The signal callback libc registers with `callback_signal`.
pub const SIGNAL_CALLBACK: &str = "__wasm_signal";

pub struct Wasix1Instance {
    procs: Arc<Procs>,
    /// The package commands v1 programs start with their defaults.
    commands: Arc<RwLock<Commands>>,
    /// For a process image: its pid and the compat state it started with (or took over from
    /// its parent, for a fork child).
    image: Mutex<Option<(TaskId, Arc<ProcState>)>>,
    exports: Exports,
    initialize: Option<Function>,
    /// `_start` of a process image, `wasi_thread_start` of a thread.
    entry: Function,
    /// The export `callback_signal` last named that exists: `__wasm_signal` unless libc
    /// named another.
    signal: Mutex<Option<Function>>,
    /// Whether this instance's thread holds signals back: `callback_signal` named an export the
    /// module does not have (see `calls::signal`).
    blocked: AtomicBool,
    /// Whether this instance's process loaded its startup state.
    started: AtomicBool,
    /// The vfork children running in this instance, by tid.
    vforks: Mutex<HashMap<TaskId, Vfork>>,
}

/// A vfork child that runs in its parent's instance.
pub(crate) struct Vfork {
    /// The compat state the child started with, a copy of its parent's.
    pub(crate) state: Arc<ProcState>,
    /// For `proc_fork` without a memory copy (asyncify builds): the parent's stack, saved at
    /// the vfork point, which the parent resumes on. `None` for `proc_fork_env` and
    /// `proc_spawn`, whose parent resumes by returning from the child's last import.
    pub(crate) resume: Option<StackId>,
}

impl Wasix1Instance {
    pub(crate) fn bind(
        procs: Arc<Procs>,
        commands: Arc<RwLock<Commands>>,
        instance: &Instance,
        task: TaskId,
        role: Role,
    ) -> Result<Self, wasmer::ExportError> {
        let exports = &instance.exports;
        let function = |name: &str| exports.get_function(name).ok().cloned();
        let (entry, image) = match role {
            Role::Process => {
                let state = Arc::new(ProcState::default());
                procs.replace(task, state.clone());
                (exports.get_function(START)?.clone(), Some((task, state)))
            }
            Role::Thread => (exports.get_function(THREAD_ENTRY)?.clone(), None),
        };
        Ok(Self {
            procs,
            commands,
            image: Mutex::new(image),
            exports: exports.clone(),
            initialize: function(INITIALIZE).filter(|_| role == Role::Process),
            entry,
            signal: Mutex::new(function(SIGNAL_CALLBACK)),
            blocked: AtomicBool::new(false),
            started: AtomicBool::new(false),
            vforks: Mutex::new(HashMap::new()),
        })
    }

    pub(crate) fn procs(&self) -> &Procs {
        &self.procs
    }

    pub(crate) fn commands(&self) -> &RwLock<Commands> {
        &self.commands
    }

    pub(crate) fn started(&self) -> bool {
        self.started.load(Ordering::Relaxed)
    }

    pub(crate) fn set_started(&self) {
        self.started.store(true, Ordering::Relaxed);
    }

    /// Makes `name` the signal callback if the instance exports a function by that name, and
    /// says whether it does; signals are held back while it does not.
    pub(crate) fn select_signal_callback(&self, name: &str) -> bool {
        let found = match self.exports.get_function(name) {
            Ok(function) => {
                *self.signal.lock().unwrap() = Some(function.clone());
                true
            }
            Err(_) => false,
        };
        self.blocked.store(!found, Ordering::Relaxed);
        found
    }

    /// Whether the instance's thread holds signals back.
    pub(crate) fn blocked(&self) -> bool {
        self.blocked.load(Ordering::Relaxed)
    }

    /// Makes `state`, a copy of the forking parent's, the compat state of this process image,
    /// a fork child whose pid is `pid`.
    pub(crate) fn adopt(&self, pid: TaskId, state: Arc<ProcState>) {
        self.procs.replace(pid, state.clone());
        let old = self.image.lock().unwrap().replace((pid, state));
        if let Some((pid, old)) = old {
            self.procs.remove(pid, &old);
        }
        self.set_started();
    }

    /// Records the vfork child `child`, which now runs in this instance.
    pub(crate) fn add_vfork(&self, child: TaskId, vfork: Vfork) {
        self.vforks.lock().unwrap().insert(child, vfork);
    }

    /// Whether `task` is a vfork child running in this instance.
    pub(crate) fn is_vfork_child(&self, task: TaskId) -> bool {
        self.vforks.lock().unwrap().contains_key(&task)
    }
}

impl Drop for Wasix1Instance {
    fn drop(&mut self) {
        if let Some((pid, state)) = self.image.get_mut().unwrap().take() {
            self.procs.remove(pid, &state);
        }
        // A vfork parent whose image ended instead of resuming leaves its child's vfork here.
        for (child, vfork) in self.vforks.get_mut().unwrap().drain() {
            self.procs.remove(child, &vfork.state);
        }
    }
}

impl<S: Send + Sync + 'static> AbiInstance<S> for Wasix1Instance {
    /// `_initialize` cannot fork: a fork child resumes in `_start`.
    fn start<'a>(&'a self, entry: &'a Entry<'a, S>) -> LocalFuture<'a, Result<(), RuntimeError>> {
        Box::pin(async move {
            if let Some(initialize) = &self.initialize {
                entry.call_once(initialize, &[]).await?;
            }
            entry.call(START, &self.entry, &[]).await.map(drop)
        })
    }

    /// WASI: returning from `_start` is `exit(0)`.
    fn on_entry_return(&self) -> EntryReturn {
        EntryReturn::Exit(0)
    }

    fn start_thread<'a>(
        &'a self,
        entry: &'a Entry<'a, S>,
        task: TaskId,
        start_arg: u32,
    ) -> LocalFuture<'a, Result<(), RuntimeError>> {
        let args = [Value::I32(task.0 as i32), Value::I32(start_arg as i32)];
        Box::pin(async move { entry.call(THREAD_ENTRY, &self.entry, &args).await.map(drop) })
    }

    /// A v1 thread that returns from its entry exited cleanly.
    fn on_thread_return(&self) -> EntryReturn {
        EntryReturn::Exit(0)
    }

    /// libc keeps the dispositions and runs the handler from its callback, `__wasm_signal(sig)`;
    /// the host only decides that it runs. The mask in force before delivery is restored
    /// afterwards. v1's handler has no frame to cancel a restart through, so `restart` stands.
    fn deliver_signal<'a>(
        &'a self,
        env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
        frame: &'a SignalFrame,
        restart: bool,
    ) -> LocalFuture<'a, Result<Sigreturn, RuntimeError>> {
        Box::pin(async move {
            let Some(callback) = self.signal.lock().unwrap().clone() else {
                return Err(RuntimeError::new(format!(
                    "a signal handler must run, but the guest has no `{SIGNAL_CALLBACK}`"
                )));
            };
            call_guest(env, &callback, &[Value::I32(frame.signo())]).await?;
            Ok(Sigreturn {
                mask: Some(frame.mask),
                restart,
            })
        })
    }

    /// A parent that vforked with `proc_fork_env` or `proc_spawn` resumes by returning from the
    /// child's `proc_exec*` or `proc_exit2` (libc then calls `__vfork_restore` itself); one that
    /// vforked with `proc_fork` rewinds the stack saved at its vfork point, so `proc_fork`
    /// returns the child's pid.
    fn vfork_resume<'a>(
        &'a self,
        env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
        child: TaskId,
    ) -> LocalFuture<'a, VforkResume> {
        Box::pin(async move {
            let Some(vfork) = self.vforks.lock().unwrap().remove(&child) else {
                return VforkResume::ReturnSuccess;
            };
            // A new image of the child has a state of its own by now, which stays.
            self.procs.remove(child, &vfork.state);
            match vfork.resume {
                None => VforkResume::ReturnSuccess,
                Some(id) => VforkResume::Unwind(
                    wasixv2_exec::asyncify::import::restore(env, id, child.0 as i32).await,
                ),
            }
        })
    }

    /// Below the scratch window, which the shims hand the kernel for translated values.
    fn memory_limit(&self) -> u64 {
        u64::from(crate::memory::SCRATCH_BASE)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
