//! Syscall entry (`syscall_trace_enter`): what the kernel does with a call before it runs it,
//! as a [`Layer`] in front of the kernel. Today that is seccomp.

use wasixv2_abi::Errno;
use wasixv2_syscalls::{Caller, Layer, SysErr, SysResult, Syscall, SyscallRet, Syscalls, dispatch};

use crate::Kernel;

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
        let traced = next.tracees.used();
        let ret = match next.syscall_entry(caller, &call).await {
            Some(ret) => ret,
            None => self.0.call(caller, call, next).await,
        };
        if traced {
            next.syscall_exit(caller, &ret).await;
        }
        ret
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

impl Kernel {
    /// What happens to `call` before it runs: `None` runs it; otherwise it is skipped, and the
    /// result is what the caller sees, or the caller was killed.
    ///
    /// The call is seen as riscv32 makes it: the table's number and its six registers. The
    /// extensions that are no syscall on Linux pass, but `thread_spawn`, which is `clone` with
    /// `CLONE_VM`, is seen as `clone` (its start argument where the stack would be).
    pub(crate) async fn syscall_entry(
        &self,
        caller: &Caller<'_>,
        call: &Syscall,
    ) -> Option<SysResult<SyscallRet>> {
        let (nr, args) = match *call {
            Syscall::ThreadSpawn {
                flags,
                start_arg,
                parent_tid,
                tls,
                child_tid,
            } => (
                NR_CLONE,
                [
                    u64::from(flags),
                    u64::from(start_arg),
                    u64::from(parent_tid.addr()),
                    u64::from(tls),
                    u64::from(child_tid.addr()),
                    0,
                ],
            ),
            _ => (call.nr()?, call.args()),
        };
        // `syscall_trace_enter`: the tracer sees the call first, then seccomp filters it,
        // unless the tracer suspended seccomp (`PTRACE_O_SUSPEND_SECCOMP`).
        if self.ptrace_syscall_entry(caller, nr, args).await {
            return Some(Err(Errno::ENOSYS.into()));
        }
        let suspended = self
            .trace_link(caller.task())
            .is_some_and(|link| link.has(wasixv2_abi::PtraceOptions::PTRACE_O_SUSPEND_SECCOMP));
        if suspended {
            return None;
        }
        self.secure_computing(caller, call, nr, args).await
    }

    /// `syscall_exit_work`: a traced task's tracer sees the call's end, unless the task is
    /// on its way out (an `exit`, a fatal signal), as Linux never returns from those.
    async fn syscall_exit(&self, caller: &Caller<'_>, ret: &SysResult<SyscallRet>) {
        let task = caller.task();
        if self.exiting_task(task) || self.trace_link(task).is_none() {
            return;
        }
        let raw = match ret {
            Ok(ret) => ret.to_raw(),
            Err(SysErr::Errno(errno)) => -i64::from(errno.0),
            // The `-ERESTART*` codes a tracer sees for an interrupted call.
            Err(SysErr::Restart) => -512,
            Err(SysErr::RestartNoIntr) => -513,
            Err(SysErr::RestartNoHand) => -514,
            Err(SysErr::RestartBlock) => -516,
        };
        self.ptrace_syscall_exit(task, raw).await;
    }
}
