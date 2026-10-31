//! Ready bodies still run within syscall entry, including its cancellation scope.

use super::*;
use std::{
    cell::Cell,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use wasixv2_syscalls::{Layer, Layered, MaybeFuture, Syscall, SyscallRet, current_task};

struct EagerProbe {
    calls: Cell<usize>,
    pending: bool,
}
impl Layer<Kernel> for EagerProbe {
    fn call<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        call: Syscall,
        next: &'a Kernel,
    ) -> MaybeFuture<'a, SysResult<SyscallRet>> {
        // Linux syscall_trace_enter ran before the selected body, with current set.
        assert_eq!(current_task(), Some(caller.task()));
        assert!(next.task_ref(caller.task(), |t| t.syscall.lock_or_recover().is_some()));
        assert!(matches!(call, Syscall::Getpid));
        self.calls.set(self.calls.get() + 1);
        if self.pending {
            MaybeFuture::Future(Box::pin(std::future::pending()))
        } else {
            MaybeFuture::Ready(Ok(SyscallRet::Getpid(42)))
        }
    }
}

#[test]
fn eager_ready_body_is_inside_entry_and_never_runs_at_construction() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory::default();
    let stack = Layered::new(
        SyscallEntry(EagerProbe {
            calls: Cell::new(0),
            pending: false,
        }),
        kernel,
    );
    let caller = Caller::new(task, &memory);
    let future = stack.getpid(&caller);
    assert_eq!(stack.layer.0.calls.get(), 0);
    assert_eq!(current_task(), None);
    assert_eq!(block_on(future), Ok(42));
    assert_eq!(stack.layer.0.calls.get(), 1);
    assert_eq!(current_task(), None);
    assert!(
        stack
            .inner
            .task_ref(task, |t| t.syscall.lock_or_recover().is_none())
    );
}

#[test]
fn cancelling_deferred_entry_clears_call_and_restores_current() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory::default();
    let stack = Layered::new(
        SyscallEntry(EagerProbe {
            calls: Cell::new(0),
            pending: true,
        }),
        kernel,
    );
    let caller = Caller::new(task, &memory);
    let mut future = stack.getpid(&caller);
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert_eq!(Pin::new(&mut future).poll(&mut cx), Poll::Pending);
    assert_eq!(current_task(), None);
    assert_eq!(stack.layer.0.calls.get(), 1);
    assert!(
        stack
            .inner
            .task_ref(task, |t| t.syscall.lock_or_recover().is_some())
    );
    drop(future);
    assert_eq!(current_task(), None);
    assert!(
        stack
            .inner
            .task_ref(task, |t| t.syscall.lock_or_recover().is_none())
    );
}
