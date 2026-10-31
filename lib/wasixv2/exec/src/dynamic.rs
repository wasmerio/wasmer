//! Store-local authority borrowed by the four generated dynamic-loader shims.
use crate::ExecEnv;
use wasixv2_abi::{Cstr, DlFlags, DlHandle, DlHandlePtr, Uptr, UptrPtr};
use wasixv2_syscalls::{Caller, DynamicLinker, Errno, LocalFuture, SysResult, UserSliceMut};
use wasmer::AsyncFunctionEnvMut;

/// An import-local adapter. Neither the kernel nor its futures retain this borrowed object.
pub struct GuestDynamicLinker<'a, S: 'static> {
    env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
}
impl<'a, S: 'static> GuestDynamicLinker<'a, S> {
    pub fn new(env: &'a AsyncFunctionEnvMut<ExecEnv<S>>) -> Self {
        Self { env }
    }
}
impl<S: 'static> DynamicLinker for GuestDynamicLinker<'_, S> {
    #[allow(clippy::too_many_arguments)]
    fn dl_open<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        file: Cstr,
        flags: DlFlags,
        search_path: Cstr,
        caller_addr: Uptr,
        handle: DlHandlePtr,
        err: UserSliceMut,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async move {
            // Snapshot the ABI without retaining the store read guard across its work.
            let abi = self
                .env
                .read()
                .await
                .data()
                .abi()
                .cloned()
                .ok_or(Errno::ENOSYS)?;
            abi.dl_open(
                self.env,
                caller,
                file,
                flags,
                search_path,
                caller_addr,
                handle,
                err,
            )
            .await
        })
    }

    fn dl_sym<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        handle: DlHandle,
        symbol: Cstr,
        caller_addr: Uptr,
        addr: UptrPtr,
        err: UserSliceMut,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async move {
            // Snapshot the ABI without retaining the store read guard across its work.
            let abi = self
                .env
                .read()
                .await
                .data()
                .abi()
                .cloned()
                .ok_or(Errno::ENOSYS)?;
            abi.dl_sym(self.env, caller, handle, symbol, caller_addr, addr, err)
                .await
        })
    }

    fn dl_close<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        handle: DlHandle,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async move {
            // Snapshot the ABI without retaining the store read guard across its work.
            let abi = self
                .env
                .read()
                .await
                .data()
                .abi()
                .cloned()
                .ok_or(Errno::ENOSYS)?;
            abi.dl_close(self.env, caller, handle).await
        })
    }

    fn dl_next_init<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        ctors: UptrPtr,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async move {
            // Snapshot the ABI without retaining the store read guard across its work.
            let abi = self
                .env
                .read()
                .await
                .data()
                .abi()
                .cloned()
                .ok_or(Errno::ENOSYS)?;
            abi.dl_next_init(self.env, caller, ctors).await
        })
    }
}

#[cfg(all(test, feature = "sys"))]
mod tests {
    use super::*;
    use crate::{AbiInstance, Entry, EntryReturn, Sigreturn, VforkResume};
    use wasixv2_syscalls::{SignalFrame, TaskId};
    use wasmer::{Function, FunctionEnv, Instance, Module, RuntimeError, Store};

    struct Probe;
    impl AbiInstance<crate::process::tests::Recorder> for Probe {
        fn dl_close<'a>(
            &'a self,
            env: &'a AsyncFunctionEnvMut<ExecEnv<crate::process::tests::Recorder>>,
            caller: &'a Caller<'_>,
            handle: DlHandle,
        ) -> LocalFuture<'a, SysResult<i32>> {
            Box::pin(async move {
                assert_eq!(caller.task(), TaskId(1));
                assert_eq!(handle.0, 9);
                // A retained adapter read guard would prevent exclusive engine access.
                assert!(env.try_write().is_some());
                Ok(77)
            })
        }
        fn start<'a>(
            &'a self,
            _: &'a Entry<'a, crate::process::tests::Recorder>,
        ) -> LocalFuture<'a, Result<(), RuntimeError>> {
            unreachable!()
        }
        fn on_entry_return(&self) -> EntryReturn {
            EntryReturn::Fault
        }
        fn start_thread<'a>(
            &'a self,
            _: &'a Entry<'a, crate::process::tests::Recorder>,
            _: i32,
            _: u32,
        ) -> LocalFuture<'a, Result<(), RuntimeError>> {
            unreachable!()
        }
        fn on_thread_return(&self) -> EntryReturn {
            EntryReturn::Fault
        }
        fn deliver_signal<'a>(
            &'a self,
            _: &'a AsyncFunctionEnvMut<ExecEnv<crate::process::tests::Recorder>>,
            _: &'a SignalFrame,
            _: bool,
        ) -> LocalFuture<'a, Result<Sigreturn, RuntimeError>> {
            unreachable!()
        }
        fn vfork_resume<'a>(
            &'a self,
            _: &'a AsyncFunctionEnvMut<ExecEnv<crate::process::tests::Recorder>>,
            _: TaskId,
            _: i32,
        ) -> LocalFuture<'a, VforkResume> {
            unreachable!()
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }
    async fn invoke(env: AsyncFunctionEnvMut<ExecEnv<crate::process::tests::Recorder>>) -> i32 {
        let memory = crate::GuestMemory::new(&env);
        let caller = Caller::new(TaskId(1), &memory);
        GuestDynamicLinker::new(&env)
            .dl_close(&caller, DlHandle(9))
            .await
            .unwrap()
    }
    #[test]
    fn borrowed_loader_releases_environment_guard_before_abi_work() {
        use crate::process::tests::{NoThreads, Recorder};
        use std::sync::Arc;
        let mut store = Store::default();
        let control = Arc::new(Recorder::default());
        let host = Arc::new(NoThreads);
        let spawner = crate::process::spawner(
            store.engine().clone(),
            control.clone(),
            host.clone(),
            Arc::new(crate::Abis::wasixv2()),
        );
        let env = FunctionEnv::new(
            &mut store,
            ExecEnv::new(
                control,
                TaskId(1),
                crate::process::ProcExports::new(spawner, Arc::default()),
                host,
            ),
        );
        env.as_mut(&mut store).abi = Some(Arc::new(Probe));
        let function = Function::new_typed_with_env_async(&mut store, &env, invoke);
        let module = Module::new(&store, wat::parse_str("(module (import \"test\" \"invoke\" (func $f (result i32))) (func (export \"run\") (result i32) call $f))").unwrap()).unwrap();
        let instance = Instance::new(
            &mut store,
            &module,
            &wasmer::imports! { "test" => { "invoke" => function } },
        )
        .unwrap();
        let run = instance.exports.get_function("run").unwrap();
        let values =
            futures::executor::block_on(run.call_async(&store.into_async(), vec![])).unwrap();
        assert_eq!(values[0].i32(), Some(77));
    }
}
