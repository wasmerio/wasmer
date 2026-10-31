//! Loader coordination only: wasm compilation, instances and initialization stay in exec/dylink.
use crate::Kernel;
use std::sync::Arc;
use wasixv2_syscalls::{
    Errno, LoaderClosing, LoaderControl, LoaderGate, LoaderTransaction, LoaderWait, LocalFuture,
    TaskId,
};
use wasixv2_wait::wait_event;

impl LoaderControl for Kernel {
    fn loader_read<'a>(
        &'a self,
        caller: &'a wasixv2_syscalls::Caller<'_>,
        path: &'a [u8],
    ) -> LocalFuture<'a, wasixv2_syscalls::SysResult<wasixv2_syscalls::LibraryImage>> {
        Box::pin(self.read_library(caller, path))
    }
    fn loader_reserve<'a>(
        &'a self,
        caller: &'a wasixv2_syscalls::Caller<'_>,
        bytes: u32,
        alignment: u32,
    ) -> LocalFuture<'a, wasixv2_syscalls::SysResult<Box<dyn wasixv2_syscalls::LoaderReservation>>>
    {
        Box::pin(async move {
            use wasixv2_abi::{Fd, MapFlags, ProtFlags};
            let mm = self.mm(caller.task());
            let memory = mm.foreign().ok_or(Errno::ENOSYS)?;
            if !alignment.is_power_of_two() {
                return Err(Errno::EINVAL.into());
            }
            let bytes = bytes.max(1);
            let len = bytes
                .checked_add(alignment - 1)
                .and_then(|n| n.checked_add(4095))
                .map(|n| n & !4095)
                .ok_or(Errno::ENOMEM)?;
            let start = self
                .sys_mmap(
                    caller,
                    0,
                    len,
                    ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                    MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS,
                    Fd(-1),
                    0,
                )
                .await?;
            let address = (u64::from(start) + u64::from(alignment - 1)) & !u64::from(alignment - 1);
            Ok(Box::new(Reservation {
                mm,
                memory,
                host: self.host.clone(),
                start,
                len,
                address: address as u32,
                bytes,
                retained: false,
            })
                as Box<dyn wasixv2_syscalls::LoaderReservation>)
        })
    }

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

    fn loader_fatal_supported(&self) -> bool {
        true
    }

    fn loader_fatal(&self, gate: &LoaderGate) -> Result<(), Errno> {
        let groups = {
            let state = self.state.read_or_recover();
            state
                .tasks
                .iter()
                .filter(|(_, entry)| {
                    std::ptr::eq(&*entry.mm.loader, gate) && !entry.thread.left_mm()
                })
                .map(|(id, entry)| (entry.process.pid.0, *id))
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        // Approved fatal IMAGE boundary: include unstarted CLONE_VM groups, even if
        // the transaction owner already exec'd/exited. Reuse kernel/exit.c:do_group_exit
        // through force_fatal_signal outside the membership lock; no guest handler runs.
        for task in groups.into_values() {
            self.force_fatal_signal(task, wasixv2_abi::Signal::SIGABRT.0);
        }
        Ok(())
    }

    fn loader_held(&self, task: TaskId) -> bool {
        let signals = self.task_signals(task);
        signals.held()
            || signals
                .throttled_until()
                .is_some_and(|until| self.host.monotonic() < until)
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

struct Reservation {
    mm: Arc<crate::mm::Mm>,
    memory: Arc<dyn wasixv2_syscalls::UserMemory + Send + Sync>,
    host: Arc<dyn wasixv2_host::Host>,
    start: u32,
    len: u32,
    address: u32,
    bytes: u32,
    retained: bool,
}

async fn release(
    mm: Arc<crate::mm::Mm>,
    memory: Arc<dyn wasixv2_syscalls::UserMemory + Send + Sync>,
    start: u32,
    len: u32,
) {
    let _op = mm.op.lock().await;
    mm.release_range(
        &*memory,
        u64::from(start),
        u64::from(start) + u64::from(len),
    )
    .await;
}

impl wasixv2_syscalls::LoaderReservation for Reservation {
    fn address(&self) -> u32 {
        self.address
    }
    fn length(&self) -> u32 {
        self.bytes
    }
    fn retain(&mut self) {
        self.retained = true;
    }
    fn release(mut self: Box<Self>) -> LocalFuture<'static, ()> {
        self.retained = true;
        Box::pin(release(
            self.mm.clone(),
            self.memory.clone(),
            self.start,
            self.len,
        ))
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if !self.retained {
            // Cancellation transfers cleanup to the existing executor, keyed by mm Arc;
            // no service, guest code, or syscall through a possibly recycled tid is involved.
            self.host.executor().spawn_local(Box::pin(release(
                self.mm.clone(),
                self.memory.clone(),
                self.start,
                self.len,
            )));
        }
    }
}
