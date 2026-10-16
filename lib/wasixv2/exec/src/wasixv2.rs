//! The native `wasixv2` guest ABI: the generated import table, `_start`, the thread entry, the
//! riscv32-shaped signal frame and the vfork return export.

use std::{any::Any, sync::Arc};

use wasixv2_syscalls::{Control, LocalFuture, SignalFrame, Syscalls, TaskId};
use wasmer::{
    AsyncFunctionEnvMut, Function, FunctionEnv, Imports, Instance, Module, RuntimeError, Store,
    StoreAsync, Value,
};

use crate::{
    ExecEnv, ExecError,
    abi::{Abi, AbiInstance, EntryReturn, Role, VforkResume},
    generated, process, signal, thread,
};

/// The `wasixv2` ABI: modules that import from [`crate::IMPORT_MODULE`].
pub struct Wasixv2;

impl<S> Abi<S> for Wasixv2
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    fn name(&self) -> &'static str {
        generated::IMPORT_MODULE
    }

    fn matches(&self, module: &Module) -> bool {
        module
            .imports()
            .any(|import| import.module() == generated::IMPORT_MODULE)
    }

    fn imports(&self, store: &mut Store, env: &FunctionEnv<ExecEnv<S>>) -> Imports {
        generated::imports(store, env)
    }

    fn bind(
        &self,
        instance: &Instance,
        _store: &mut Store,
        _task: TaskId,
        role: Role,
    ) -> Result<Arc<dyn AbiInstance<S>>, ExecError> {
        let exports = &instance.exports;
        let entry = match role {
            Role::Process => exports.get_function("_start")?,
            Role::Thread => exports.get_function(thread::THREAD_ENTRY)?,
        };
        Ok(Arc::new(Wasixv2Instance {
            entry: entry.clone(),
            signal: signal::SignalExports::new(instance),
            vfork_return: exports.get_function(process::VFORK_RETURN).ok().cloned(),
        }))
    }
}

struct Wasixv2Instance {
    /// `_start` of a process image, `wasixv2_thread_start` of a thread.
    entry: Function,
    signal: signal::SignalExports,
    /// `wasixv2_vfork_return(child)`, which libc exports to unwind to the vfork point.
    vfork_return: Option<Function>,
}

impl<S: Control + 'static> AbiInstance<S> for Wasixv2Instance {
    fn start(&self, store: &StoreAsync) -> LocalFuture<'static, Result<(), RuntimeError>> {
        let call = self.entry.call_async(store, Vec::new());
        Box::pin(async move { call.await.map(drop) })
    }

    // The guest must end with `exit_group`; Linux faults a process whose entry returns.
    fn on_entry_return(&self) -> EntryReturn {
        EntryReturn::Fault
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

    fn on_thread_return(&self) -> EntryReturn {
        EntryReturn::Fault
    }

    fn deliver_signal<'a>(
        &'a self,
        env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
        frame: &'a SignalFrame,
    ) -> LocalFuture<'a, Result<Option<u64>, RuntimeError>> {
        Box::pin(async move { signal::deliver(env, &self.signal, frame).await.map(Some) })
    }

    fn vfork_resume<'a>(
        &'a self,
        env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
        child: TaskId,
    ) -> LocalFuture<'a, VforkResume> {
        Box::pin(async move {
            VforkResume::Unwind(process::vfork_return(env, self.vfork_return.as_ref(), child).await)
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
