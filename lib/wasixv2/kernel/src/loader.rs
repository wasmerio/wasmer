//! Loader coordination only: wasm compilation, instances and initialization stay in exec/dylink.
use crate::Kernel;
use std::sync::Arc;
use wasixv2_syscalls::{
    Errno, LoaderClosing, LoaderControl, LoaderGate, LoaderTransaction, LoaderWait, LocalFuture,
    TaskId,
};
use wasixv2_wait::wait_event;

impl LoaderControl for Kernel {
    fn loader_gate(&self, task: TaskId) -> Option<Arc<LoaderGate>> {
        self.state
            .read_or_recover()
            .tasks
            .get(&task)
            .map(|task| task.mm.loader.clone())
    }

    fn loader_activate(
        &self,
        task: TaskId,
        closing: LoaderClosing,
    ) -> Result<LoaderTransaction, Errno> {
        // Check before taking the vfork snapshot: an in-flight clone may still be
        // publishing its borrowed instance. Once drained, closed admission prevents
        // another one from appearing while we acquire the existing lock order.
        if !closing.gate().ready_for(LoaderWait::Drained) {
            return Err(Errno::EBUSY);
        }
        let borrowed = self.vfork_parents();
        let state = self.state.lock_or_recover();
        let task = state.tasks.get(&task).ok_or(Errno::ESRCH)?;
        if !Arc::ptr_eq(&task.mm.loader, closing.gate()) {
            return Err(Errno::ESTALE);
        }
        let members = state
            .tasks
            .iter()
            .filter(|(_, t)| Arc::ptr_eq(&t.mm, &task.mm) && !t.thread.left_mm())
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        if borrowed.iter().any(|task| members.contains(task)) {
            return Err(Errno::EBUSY);
        }
        // Task detachment and clone admission cannot interleave this membership snapshot.
        let round = closing.activate(&members)?;
        let notify = members
            .into_iter()
            .map(|member| (member, state.tasks[&member].signals.clone()))
            .collect::<Vec<_>>();
        drop(state);
        for (member, signals) in notify {
            signals.group().notify(member);
        }
        Ok(round)
    }

    fn loader_notify(&self, task: TaskId) {
        self.task_signals(task).group().notify(task);
    }

    fn loader_sleep<'a>(
        &'a self,
        task: TaskId,
        gate: &'a LoaderGate,
        wait: LoaderWait,
    ) -> LocalFuture<'a, Result<(), Errno>> {
        Box::pin(async move {
            // kernel/task_work.c:task_work_run's sibling wait: only fatal signals cancel
            // a parked loader participant. Ordinary handlers run after publication/release.
            let signals = self.task_signals(task);
            wait_event(
                gate.queue(),
                || gate.ready_for(wait).then_some(()),
                &signals.killable(),
            )
            .await
            .map_err(|_| Errno::EINTR)
        })
    }
}
