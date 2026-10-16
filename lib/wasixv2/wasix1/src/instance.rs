//! The per-instance half of the v1 ABI: entry points, signal delivery and vfork resumption as
//! wasix-libc expects them.

use std::{
    any::Any,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use wasixv2_exec::{AbiInstance, EntryReturn, ExecEnv, Role, VforkResume};
use wasixv2_syscalls::{LocalFuture, SignalFrame, TaskId};
use wasmer::{AsyncFunctionEnvMut, Function, Instance, RuntimeError, StoreAsync, Value};

use crate::process::{ProcState, Procs};

/// `_initialize`, which reactors and some commands export; it runs before `_start`.
const INITIALIZE: &str = "_initialize";
const START: &str = "_start";
/// The thread entry, as `wasi_thread_start(tid, start_arg)`.
pub const THREAD_ENTRY: &str = "wasi_thread_start";
/// The signal callback libc registers with `callback_signal`.
pub const SIGNAL_CALLBACK: &str = "__wasm_signal";

pub struct Wasix1Instance {
    procs: Arc<Procs>,
    /// For a process image: its pid and the compat state it started.
    image: Option<(TaskId, Arc<ProcState>)>,
    initialize: Option<Function>,
    /// `_start` of a process image, `wasi_thread_start` of a thread.
    entry: Function,
    signal: Option<Function>,
    /// Whether this instance's process loaded its startup state.
    started: AtomicBool,
}

impl Wasix1Instance {
    pub(crate) fn bind(
        procs: Arc<Procs>,
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
            image,
            initialize: function(INITIALIZE).filter(|_| role == Role::Process),
            entry,
            signal: function(SIGNAL_CALLBACK),
            started: AtomicBool::new(false),
        })
    }

    pub(crate) fn procs(&self) -> &Procs {
        &self.procs
    }

    pub(crate) fn started(&self) -> bool {
        self.started.load(Ordering::Relaxed)
    }

    pub(crate) fn set_started(&self) {
        self.started.store(true, Ordering::Relaxed);
    }
}

impl Drop for Wasix1Instance {
    fn drop(&mut self) {
        if let Some((pid, state)) = &self.image {
            self.procs.remove(*pid, state);
        }
    }
}

impl<S: 'static> AbiInstance<S> for Wasix1Instance {
    fn start(&self, store: &StoreAsync) -> LocalFuture<'static, Result<(), RuntimeError>> {
        let initialize = self
            .initialize
            .as_ref()
            .map(|function| function.call_async(store, Vec::new()));
        let start = self.entry.call_async(store, Vec::new());
        Box::pin(async move {
            if let Some(initialize) = initialize {
                initialize.await?;
            }
            start.await.map(drop)
        })
    }

    /// WASI: returning from `_start` is `exit(0)`.
    fn on_entry_return(&self) -> EntryReturn {
        EntryReturn::Exit(0)
    }

    fn start_thread(
        &self,
        store: &StoreAsync,
        task: TaskId,
        start_arg: u32,
    ) -> LocalFuture<'static, Result<(), RuntimeError>> {
        let args = vec![Value::I32(task.0 as i32), Value::I32(start_arg as i32)];
        let call = self.entry.call_async(store, args);
        Box::pin(async move { call.await.map(drop) })
    }

    /// A v1 thread that returns from its entry exited cleanly.
    fn on_thread_return(&self) -> EntryReturn {
        EntryReturn::Exit(0)
    }

    /// libc keeps the dispositions and runs the handler from `__wasm_signal(sig)`; the host only
    /// decides that it runs. The mask in force before delivery is restored afterwards.
    fn deliver_signal<'a>(
        &'a self,
        env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
        frame: &'a SignalFrame,
    ) -> LocalFuture<'a, Result<Option<u64>, RuntimeError>> {
        Box::pin(async move {
            let Some(callback) = &self.signal else {
                return Err(RuntimeError::new(format!(
                    "a signal handler must run, but the guest has no `{SIGNAL_CALLBACK}`"
                )));
            };
            let args = vec![Value::I32(frame.signo())];
            callback.call_async(&env.as_store_async(), args).await?;
            Ok(Some(frame.mask))
        })
    }

    /// v1 resumes a vfork parent by returning from the child's `proc_exec*` or `proc_exit2`;
    /// libc then calls `__vfork_restore` itself.
    fn vfork_resume<'a>(
        &'a self,
        _env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
        _child: TaskId,
    ) -> LocalFuture<'a, VforkResume> {
        Box::pin(async { VforkResume::ReturnSuccess })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
