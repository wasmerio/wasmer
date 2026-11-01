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
}
