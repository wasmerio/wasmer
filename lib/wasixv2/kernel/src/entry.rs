//! Syscall entry (`syscall_trace_enter`): what the kernel does with a call before it runs it,
//! as a [`Layer`] in front of the kernel: the current task and seccomp.

use wasixv2_abi::Errno;
use wasixv2_syscalls::{Caller, Layer, SysErr, SysResult, Syscall, SyscallRet, Syscalls, dispatch};

use crate::{Kernel, ptrace::CallInfo, sync::MutexExt as _};
use std::sync::{Arc, Mutex};

/// The syscall visible through /proc, even for an untraced task.
pub(crate) type CallSlot = Arc<Mutex<Option<CallInfo>>>;

struct CurrentCall(CallSlot);

impl Drop for CurrentCall {
    fn drop(&mut self) {
        *self.0.lock_or_recover() = None;
    }
}

fn call_regs(call: &Syscall) -> Option<(u32, [u64; 6])> {
    match *call {
        Syscall::ThreadSpawn {
            flags,
            start_arg,
            parent_tid,
            tls,
            child_tid,
        } => Some((
            NR_CLONE,
            [
                u64::from(flags),
                u64::from(start_arg),
                u64::from(parent_tid.addr()),
                u64::from(tls),
                u64::from(child_tid.addr()),
                0,
            ],
        )),
        _ => Some((call.nr()?, call.args())),
    }
}

/// `clone`'s riscv32 number, which `thread_spawn` is seen as.
const NR_CLONE: u32 = 220;

/// The kernel's syscall entry: runs [`Kernel::syscall_entry`] before each call, then hands
/// the calls it lets through to the layer `L` in front of the kernel (a tracer, or
/// [`Dispatch`]).
///
/// It wraps that layer rather than being stacked as one more [`Layered`]: each layer nests the
/// futures of every call once more, which debug builds pay for in stack.
///
/// [`Layered`]: wasixv2_syscalls::Layered
#[derive(Clone, Copy, Debug, Default)]
pub struct SyscallEntry<L = Dispatch>(pub L);

impl<L: Layer<Kernel>> Layer<Kernel> for SyscallEntry<L> {
    async fn call(
        &self,
        caller: &Caller<'_>,
        call: Syscall,
        next: &Kernel,
    ) -> SysResult<SyscallRet> {
        wasixv2_syscalls::as_task(caller.task(), async {
            // fs/proc/base.c `proc_pid_syscall`: retain the call while suspended, including
            // syscall-exit tracing. Drop clears it on both completion and cancellation.
            let slot = next.task_ref(caller.task(), |t| t.syscall.clone());
            *slot.lock_or_recover() = call_regs(&call).map(|(nr, args)| CallInfo {
                nr,
                args,
                ..Default::default()
            });
            let _current = CurrentCall(slot);
            let traced = next.tracees.used();
            let ret = match next.syscall_entry(caller, &call).await {
                Entry::Done(ret) => ret,
                Entry::Run(changed) => self.0.call(caller, changed, next).await,
            };
            let ret = if traced {
                next.syscall_exit(caller, &call, ret).await
            } else {
                ret
            };
            // The typed wasm import keeps its return type even if ptrace changed the syscall.
            ret.map(|ret| call.ret_from_raw(ret.to_raw()))
        })
        .await
    }
}

/// The layer that only runs the call.
#[derive(Clone, Copy, Debug, Default)]
pub struct Dispatch;

impl<S: Syscalls> Layer<S> for Dispatch {
    async fn call(&self, caller: &Caller<'_>, call: Syscall, next: &S) -> SysResult<SyscallRet> {
        dispatch(next, caller, call).await
    }
}

enum Entry {
    Run(Syscall),
    Done(SysResult<SyscallRet>),
}

impl Kernel {
    /// The syscall registers after a tracer stop (kernel/entry/common.c syscall_trace_enter).
    fn ptrace_regs(&self, task: wasixv2_syscalls::TaskId) -> Option<crate::ptrace::CallInfo> {
        if !self.tracees.used() {
            return None;
        }
        self.task_signals(task)
            .group()
            .with_trace(task, |t| t.call.filter(|_| t.call_set))
            .flatten()
    }

    /// Syscall entry after Linux's syscall_trace_enter: the tracer can replace or skip the
    /// call, then seccomp sees its new registers, and can stop it for another tracer edit.
    ///
    /// The call is seen as riscv32 makes it: the table's number and its six registers. The
    /// extensions that are no syscall on Linux pass, but `thread_spawn`, which is `clone` with
    /// `CLONE_VM`, is seen as `clone` (its start argument where the stack would be).
    async fn syscall_entry(&self, caller: &Caller<'_>, call: &Syscall) -> Entry {
        let Some((nr, args)) = call_regs(call) else {
            return Entry::Run(*call);
        };
        // `syscall_trace_enter`: the tracer sees the call first, then seccomp filters it,
        // unless the tracer suspended seccomp (`PTRACE_O_SUSPEND_SECCOMP`).
        if self.ptrace_syscall_entry(caller, nr, args).await {
            return Entry::Done(Err(Errno::ENOSYS.into()));
        }
        let (nr, args) = self
            .ptrace_regs(caller.task())
            .map_or((nr, args), |r| (r.nr, r.args));
        if nr == u32::MAX {
            return Entry::Done(Err(Errno::ENOSYS.into()));
        }
        let suspended = self
            .trace_link(caller.task())
            .is_some_and(|link| link.has(wasixv2_abi::PtraceOptions::PTRACE_O_SUSPEND_SECCOMP));
        if !suspended && let Some(ret) = self.secure_computing(caller, call, nr, args).await {
            return Entry::Done(ret);
        }
        // Decode after filtering: seccomp also sees unknown syscall numbers and may deny
        // them with its own errno before dispatch would return ENOSYS.
        match self.ptrace_regs(caller.task()) {
            Some(regs) => match Syscall::from_regs(regs.nr, regs.args) {
                Some(call) => Entry::Run(call),
                None => Entry::Done(Err(Errno::ENOSYS.into())),
            },
            None => Entry::Run(*call),
        }
    }

    /// `syscall_exit_work`: a traced task's tracer sees the call's end, unless the task is
    /// on its way out (an `exit`, a fatal signal), as Linux never returns from those.
    async fn syscall_exit(
        &self,
        caller: &Caller<'_>,
        call: &Syscall,
        ret: SysResult<SyscallRet>,
    ) -> SysResult<SyscallRet> {
        let task = caller.task();
        if self.exiting_task(task) || self.trace_link(task).is_none() {
            return ret;
        }
        let raw = match &ret {
            Ok(ret) => ret.to_raw(),
            Err(SysErr::Errno(errno)) => -i64::from(errno.0),
            // The `-ERESTART*` codes a tracer sees for an interrupted call.
            Err(SysErr::Restart) => -512,
            Err(SysErr::RestartNoIntr) => -513,
            Err(SysErr::RestartNoHand) => -514,
            Err(SysErr::RestartBlock) => -516,
        };
        self.ptrace_syscall_exit(task, raw).await;
        let changed = self
            .task_signals(task)
            .group()
            .with_trace(task, |t| {
                t.call.filter(|_| t.call_set).and_then(|call| call.ret)
            })
            .flatten();
        match changed {
            Some(raw) if (-4095..0).contains(&raw) => Err(Errno(-raw as i32).into()),
            Some(raw) => Ok(call.ret_from_raw(raw)),
            None => ret,
        }
    }
}
