use crate::memory::UserMemory;

/// Identifies a task (a thread, in Linux terms) inside one kernel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TaskId(pub u32);

/// The task making a syscall and the memory its pointers refer to.
pub struct Caller<'a> {
    task: TaskId,
    memory: &'a dyn UserMemory,
}

impl<'a> Caller<'a> {
    pub fn new(task: TaskId, memory: &'a dyn UserMemory) -> Self {
        Self { task, memory }
    }

    pub fn task(&self) -> TaskId {
        self.task
    }

    pub fn memory(&self) -> &'a dyn UserMemory {
        self.memory
    }
}

std::thread_local! {
    static CURRENT: std::cell::Cell<Option<TaskId>> = const { std::cell::Cell::new(None) };
}

/// The task whose syscall is being polled on this thread (Linux's `current`), for what learns of
/// a change far below the syscall that made it: fanotify reports who caused an event. `None`
/// outside any task's syscall, as for work the kernel does on its own.
pub fn current_task() -> Option<TaskId> {
    CURRENT.with(std::cell::Cell::get)
}

/// `future` run as `task`'s: while it is polled, [`current_task`] is `task`. The kernel's
/// syscall entry scopes each call, including calls from a vfork child on its parent's instance.
pub fn as_task<F: std::future::Future>(task: TaskId, future: F) -> AsTask<F> {
    AsTask {
        task,
        future: Box::pin(future),
    }
}

/// See [`as_task`]. Large syscall futures stay behind a stable, compact pointer.
pub struct AsTask<F> {
    task: TaskId,
    future: std::pin::Pin<Box<F>>,
}

impl<F: std::future::Future> std::future::Future for AsTask<F> {
    type Output = F::Output;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<F::Output> {
        struct Restore(Option<TaskId>);
        impl Drop for Restore {
            fn drop(&mut self) {
                CURRENT.with(|current| current.set(self.0));
            }
        }
        // Linux include/linux/sched.h:current names the executing task. Keep
        // that scope through nesting and restore it even when a poll unwinds.
        let _restore = Restore(CURRENT.with(|current| current.replace(Some(self.task))));
        self.future.as_mut().poll(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_is_current_only_while_its_run_is_polled() {
        assert_eq!(current_task(), None);
        let inner = as_task(TaskId(7), async { current_task() });
        let outer = as_task(TaskId(3), async move { (current_task(), inner.await) });
        let seen = futures_executor_block_on(outer);
        assert_eq!(seen, (Some(TaskId(3)), Some(TaskId(7))));
        assert_eq!(current_task(), None);
    }

    #[test]
    fn a_panicking_task_restores_the_outer_current_task() {
        let outer = as_task(TaskId(3), async {
            let panic = std::panic::catch_unwind(|| {
                futures_executor_block_on(as_task(TaskId(7), async {
                    assert_eq!(current_task(), Some(TaskId(7)));
                    panic!("guest operation failed");
                }));
            });
            assert!(panic.is_err());
            current_task()
        });
        let seen = futures_executor_block_on(outer);
        assert_eq!(current_task(), None);
        assert_eq!(seen, Some(TaskId(3)));
    }

    /// Polls a future that never waits to its end.
    fn futures_executor_block_on<F: std::future::Future>(future: F) -> F::Output {
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        let mut future = std::pin::pin!(future);
        loop {
            if let std::task::Poll::Ready(out) = future.as_mut().poll(&mut cx) {
                return out;
            }
        }
    }
}
