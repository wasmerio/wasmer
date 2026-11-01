//! Custody of an engine error while ordinary syscall layers finish an import.

use std::cell::RefCell;

use wasixv2_syscalls::{Control, SysErr, SysResult};
use wasmer::{AsyncFunctionEnvMut, RuntimeError};

use crate::ExecEnv;

/// One import invocation's original guest error. Nested imports own separate objects.
/// This object is borrowed by engine capabilities and never retained by the kernel.
#[derive(Default)]
pub struct GuestUnwind {
    state: RefCell<State>,
}

#[derive(Default)]
enum State {
    #[default]
    Empty,
    Raised(RuntimeError),
    Consumed,
}

impl GuestUnwind {
    /// Retains the first original error and returns its engine-free dispatch marker.
    /// A layer cannot replace that error by changing the syscall result afterward.
    pub fn raise<T>(&self, error: RuntimeError) -> SysResult<T> {
        let mut state = self.state.borrow_mut();
        if matches!(*state, State::Empty) {
            *state = State::Raised(error);
        }
        Err(SysErr::GuestUnwind)
    }

    /// Checks custody before the ordinary return-to-guest path. On unwind, ends CPU
    /// accounting exactly once and returns the same engine error without running guest work.
    pub async fn finish<S: Control + 'static, T>(
        &self,
        env: &AsyncFunctionEnvMut<ExecEnv<S>>,
        ret: SysResult<T>,
    ) -> Result<SysResult<T>, RuntimeError> {
        match self.resolve(ret) {
            Ok(ret) => Ok(ret),
            Err((error, finish)) => {
                if finish {
                    crate::finish_import(env).await;
                }
                Err(error)
            }
        }
    }

    fn resolve<T>(&self, ret: SysResult<T>) -> Result<SysResult<T>, (RuntimeError, bool)> {
        let mut state = self.state.borrow_mut();
        match std::mem::replace(&mut *state, State::Consumed) {
            State::Raised(error) => Err((error, true)),
            State::Consumed => Err((
                RuntimeError::new("an import consumed guest unwind custody more than once"),
                false,
            )),
            State::Empty if matches!(ret, Err(SysErr::GuestUnwind)) => Err((
                RuntimeError::new("guest unwind marker has no import-owned error"),
                true,
            )),
            State::Empty => {
                // A restart did not execute guest code. The same import may be dispatched again.
                *state = State::Empty;
                Ok(ret)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use wasixv2_syscalls::Errno;

    #[derive(Debug, thiserror::Error)]
    #[error("guest payload")]
    struct Payload(Arc<()>);

    #[test]
    fn original_error_survives_layer_replacement_and_nested_imports() {
        let payload = Arc::new(());
        let outer = GuestUnwind::default();
        let inner = GuestUnwind::default();
        let _: SysResult<()> = outer.raise(RuntimeError::user(Box::new(Payload(payload.clone()))));
        let _: SysResult<()> = inner.raise(RuntimeError::new("nested error"));
        assert_eq!(
            inner.resolve(Ok(())).unwrap_err().0.message(),
            "nested error"
        );
        let (error, finish) = outer.resolve::<()>(Err(Errno::EINVAL.into())).unwrap_err();
        assert!(finish);
        assert!(Arc::ptr_eq(
            &error.downcast_ref::<Payload>().unwrap().0,
            &payload
        ));
        assert!(!outer.resolve(Ok(())).unwrap_err().1);
    }

    #[test]
    fn orphan_marker_fails_and_ordinary_restart_keeps_empty_custody() {
        let unwind = GuestUnwind::default();
        assert_eq!(
            unwind.resolve::<()>(Err(SysErr::Restart)).unwrap(),
            Err(SysErr::Restart)
        );
        let (_, finish) = unwind.resolve::<()>(Err(SysErr::GuestUnwind)).unwrap_err();
        assert!(finish);
        assert!(!unwind.resolve(Ok(())).unwrap_err().1);
    }

    #[cfg(feature = "sys")]
    #[test]
    fn engine_import_keeps_original_error_and_finishes_cpu_before_work() {
        use crate::process::tests::{NoThreads, Recorder};
        use std::time::Duration;
        use wasixv2_syscalls::{
            Caller, CpuTimes, Layer, Layered, MaybeFuture, Syscall, SyscallRet, Syscalls, TaskId,
        };
        use wasmer::{Function, FunctionEnv, Instance, Module, Store};

        struct Replace {
            clocks: Arc<crate::cputime::tests::Clocks>,
            payload: Arc<()>,
        }
        impl<S: Syscalls> Layer<S> for Replace {
            fn call<'a>(
                &'a self,
                caller: &'a Caller<'_>,
                call: Syscall,
                next: &'a S,
            ) -> MaybeFuture<'a, SysResult<SyscallRet>> {
                wasixv2_syscalls::dispatch(next, caller, call).map(|_| Err(Errno::EINVAL.into()))
            }
        }
        struct Throw<'a>(&'a GuestUnwind, Arc<()>);
        impl Syscalls for Throw<'_> {
            fn getpid<'a>(&'a self, _: &'a Caller<'_>) -> MaybeFuture<'a, SysResult<i32>> {
                MaybeFuture::Ready(
                    self.0
                        .raise(RuntimeError::user(Box::new(Payload(self.1.clone())))),
                )
            }
        }
        async fn invoke(
            env: AsyncFunctionEnvMut<ExecEnv<Layered<Replace, Recorder>>>,
        ) -> Result<i32, RuntimeError> {
            let (syscalls, task) = crate::caller_parts(&env).await;
            let unwind = GuestUnwind::default();
            let memory = crate::GuestMemory::new(&env);
            let caller = Caller::new(task, &memory);
            // Work must remain pending when the callee already raised its exception.
            let _ = syscalls.inner.exit_group(&caller, 42).await;
            syscalls
                .layer
                .clocks
                .advance(Duration::from_micros(2), Duration::from_micros(2));
            let throw = Throw(&unwind, syscalls.layer.payload.clone());
            let ret = syscalls.layer.call(&caller, Syscall::Getpid, &throw).await;
            let ret = ret.map(|_| 0);
            unwind
                .finish(&env, ret)
                .await?
                .map_err(|_| RuntimeError::new("layer replacement escaped"))
        }

        let (clocks, account, sample) = crate::cputime::tests::setup(true);
        let payload = Arc::new(());
        let syscalls = Arc::new(Layered::new(
            Replace {
                clocks: clocks.clone(),
                payload: payload.clone(),
            },
            Recorder::default(),
        ));
        let host = Arc::new(NoThreads);
        let mut store = Store::default();
        let spawner = crate::process::spawner(
            store.engine().clone(),
            syscalls.clone(),
            host.clone(),
            Arc::new(crate::Abis::wasixv2()),
        );
        let env = FunctionEnv::new(
            &mut store,
            ExecEnv::new(
                syscalls.clone(),
                TaskId(1),
                crate::process::ProcExports::new(spawner, Arc::default()),
                host,
            ),
        );
        env.as_mut(&mut store).cpu = sample;
        clocks.advance(Duration::from_micros(3), Duration::from_micros(3));
        let import = Function::new_typed_with_env_async(&mut store, &env, invoke);
        let module = Module::new(&store, wat::parse_str(r#"(module (import "proof" "invoke" (func $invoke (result i32))) (func (export "run") (result i32) call $invoke))"#).unwrap()).unwrap();
        let instance = Instance::new(
            &mut store,
            &module,
            &wasmer::imports! { "proof" => { "invoke" => import } },
        )
        .unwrap();
        let function = instance.exports.get_function("run").unwrap();
        let store = store.into_async();
        let error = futures::executor::block_on(function.call_async(&store, vec![])).unwrap_err();
        assert!(Arc::ptr_eq(
            &error.downcast_ref::<Payload>().unwrap().0,
            &payload
        ));
        clocks.advance(Duration::from_micros(7), Duration::from_micros(7));
        env.as_ref(&store.read()).sample_cpu();
        assert_eq!(
            account.times(),
            CpuTimes {
                user: Duration::from_micros(10),
                system: Duration::from_micros(2)
            }
        );
        assert!(matches!(
            syscalls.take_pending(TaskId(1)),
            Some(wasixv2_syscalls::GuestAction::Exit(42))
        ));
    }
}
