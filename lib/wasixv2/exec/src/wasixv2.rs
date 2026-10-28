//! The native `wasixv2` guest ABI: the generated import table, `_start`, the thread entry, the
//! riscv32-shaped signal frame and the vfork return export.

use std::{any::Any, sync::Arc};

use wasixv2_syscalls::{Control, LocalFuture, SignalFrame, Syscalls, TaskId};
use wasmer::{
    AsyncFunctionEnvMut, Function, FunctionEnv, Global, Imports, Instance, Module, RuntimeError,
    Store, Value,
};

use crate::{
    ExecEnv, ExecError,
    abi::{Abi, AbiInstance, Entry, EntryReturn, Role, Sigreturn, VforkResume},
    asyncify, fork, generated, process, signal, thread,
};

/// The `wasixv2` ABI: modules that import from [`crate::IMPORT_MODULE`].
pub struct Wasixv2;

/// Whether `wasm` targets the `wasixv2` ABI: it imports something from the `wasixv2` module. Only
/// the import section is parsed; a malformed module is `false`.
pub fn is_wasixv2_module(wasm: &[u8]) -> bool {
    use wasmparser::{Parser, Payload};

    for payload in Parser::new(0).parse_all(wasm) {
        match payload {
            Ok(Payload::ImportSection(imports)) => {
                return imports
                    .into_imports()
                    .any(|import| import.is_ok_and(|i| i.module == generated::IMPORT_MODULE));
            }
            // Sections are ordered, so the imports are behind us once code-related ones start.
            Ok(Payload::FunctionSection(_) | Payload::CodeSectionStart { .. }) | Err(_) => {
                return false;
            }
            Ok(_) => {}
        }
    }
    false
}

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

    fn matches_image(&self, image: &[u8]) -> bool {
        is_wasixv2_module(image)
    }

    fn imports(
        &self,
        store: &mut Store,
        env: &FunctionEnv<ExecEnv<S>>,
        module: &Module,
    ) -> Imports {
        let mut imports = generated::imports(store, env, module);
        if asyncify::instrumented(module) {
            fork::imports(store, env, &mut imports);
        }
        imports
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

pub(crate) struct Wasixv2Instance {
    /// `_start` of a process image, `wasixv2_thread_start` of a thread.
    entry: Function,
    signal: signal::SignalExports,
    /// `wasixv2_vfork_return(child)`, which libc exports to unwind to the vfork point.
    vfork_return: Option<Function>,
}

impl Wasixv2Instance {
    /// `__tls_base`, which `CLONE_SETTLS` sets.
    pub(crate) fn tls_base(&self) -> Option<&Global> {
        self.signal.tls_base.as_ref()
    }
}

impl<S: Control + Send + Sync + 'static> AbiInstance<S> for Wasixv2Instance {
    fn start<'a>(&'a self, entry: &'a Entry<'a, S>) -> LocalFuture<'a, Result<(), RuntimeError>> {
        Box::pin(async move { entry.call("_start", &self.entry, &[]).await.map(drop) })
    }

    // The guest must end with `exit_group`; Linux faults a process whose entry returns.
    fn on_entry_return(&self) -> EntryReturn {
        EntryReturn::Fault
    }

    fn start_thread<'a>(
        &'a self,
        entry: &'a Entry<'a, S>,
        task: TaskId,
        start_arg: u32,
    ) -> LocalFuture<'a, Result<(), RuntimeError>> {
        let args = [Value::I32(task.0 as i32), Value::I32(start_arg as i32)];
        Box::pin(async move {
            entry
                .call(thread::THREAD_ENTRY, &self.entry, &args)
                .await
                .map(drop)
        })
    }

    fn on_thread_return(&self) -> EntryReturn {
        EntryReturn::Fault
    }

    fn deliver_signal<'a>(
        &'a self,
        env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
        frame: &'a SignalFrame,
        restart: bool,
    ) -> LocalFuture<'a, Result<Sigreturn, RuntimeError>> {
        Box::pin(async move {
            let (mask, restart) = signal::deliver(env, &self.signal, frame, restart).await?;
            Ok(Sigreturn {
                mask: Some(mask),
                restart,
            })
        })
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
