//! ptrace at the level of syscalls and signals: attaching, ptrace stops and what wait reports of
//! them, the tracee's memory, syscall stops, event stops, and the ends of tracer and tracee.

use futures::FutureExt as _;
use wasixv2_abi::{
    Idtype, PtraceEvent, PtraceOptions, PtraceRequest as R, RusagePtr, S32Ptr, SiCode, Siginfo,
    Signal, WaitOptions,
};
use wasixv2_syscalls::{Layered, read_value};

use super::{
    introspect::SyncMemory,
    ipc::{errno, get, put},
    proc::{ForkableMemory, fork_child},
    *,
};

const SIGTRAP: i32 = Signal::SIGTRAP.0;
const WALL: u32 = WaitOptions::__WALL.0;
const WNOHANG: u32 = WaitOptions::WNOHANG.0;
const NR_GETPID: u64 = 172;

type Stack = Layered<SyscallEntry, Kernel>;

fn machine() -> (Stack, TaskId, ForkableMemory) {
    let (kernel, task, _) = setup(b"");
    let memory = ForkableMemory(FlatMemory(RefCell::new(vec![0; 65536])));
    (Layered::new(SyscallEntry(Dispatch), kernel), task, memory)
}

fn flat() -> FlatMemory {
    FlatMemory(RefCell::new(vec![0; 65536]))
}

fn ptrace(
    kernel: &Kernel,
    caller: &Caller<'_>,
    request: R,
    pid: TaskId,
    addr: u32,
    data: u32,
) -> SysResult<i32> {
    block_on(kernel.ptrace(caller, request, pid.0 as i32, addr, data))
}

/// `waitpid(pid, WNOHANG | options)` as libc2 makes it (`waitid` with `WEXITED`): the
/// report's code, status and pid, or `None`.
fn wait(
    kernel: &Kernel,
    caller: &Caller<'_>,
    pid: TaskId,
    options: u32,
) -> Option<(SiCode, i32, i32)> {
    let infop = GuestPtrMut::new(0x500);
    let options = WaitOptions(options | WNOHANG | WaitOptions::WEXITED.0);
    let which = kernel.waitid(
        caller,
        Idtype::P_PID,
        pid.0 as i32,
        infop,
        options,
        RusagePtr::NULL,
    );
    block_on(which).unwrap();
    let info: Siginfo = block_on(read_value(caller.memory(), GuestPtr::new(0x500))).unwrap();
    let field = |at: usize| i32::from_le_bytes(info.sifields[at..at + 4].try_into().unwrap());
    (field(0) != 0).then(|| (SiCode(info.si_code), field(8), field(0)))
}

/// libc2's `__emulate_wait4` status for a stop (`CLD_STOPPED`, `CLD_TRAPPED`): the event bits
/// of `si_status` stay above the signal.
fn wait4_status(si_status: i32) -> i32 {
    (si_status << 8) + 0x7f
}

/// Whether `task` waits in a stop.
fn waits_stopped(kernel: &Kernel, task: TaskId) -> bool {
    Box::pin(kernel.stop_wait(task)).now_or_never().is_none()
}

/// A child of `parent` that runs on memory of its own, traced the old way
/// (`PTRACE_TRACEME`) and stopped by SIGSTOP, as strace's children start.
fn traced_child(kernel: &Kernel, caller: &Caller<'_>, child_memory: &FlatMemory) -> TaskId {
    let child = fork_child(kernel, caller);
    let child_caller = Caller::new(child, child_memory);
    assert_eq!(
        ptrace(kernel, &child_caller, R::PTRACE_TRACEME, TaskId(0), 0, 0),
        Ok(0)
    );
    kernel.kill(child, Signal::SIGSTOP).unwrap();
    assert_eq!(kernel.take_pending(child), Some(GuestAction::Stopped));
    let stopped = wait(kernel, caller, child, WALL);
    assert_eq!(
        stopped.map(|(code, status, _)| (code, status)),
        Some((SiCode::CLD_TRAPPED, Signal::SIGSTOP.0))
    );
    child
}

/// `PTRACE_TRACEME`, then a signal-delivery stop: the tracer's wait reports `CLD_TRAPPED` with
/// the signal, once, without `WUNTRACED`; the tracer reads the `siginfo`, and its `PTRACE_CONT`
/// decides what is delivered: nothing, or another signal.
#[test]
fn a_tracee_stops_for_each_signal_and_delivers_what_the_tracer_chose() {
    let (stack, parent, memory) = machine();
    let kernel = &stack.inner;
    let caller = Caller::new(parent, &memory);
    let child_memory = flat();
    let child = traced_child(kernel, &caller, &child_memory);
    let child_caller = Caller::new(child, &child_memory);
    assert_eq!(
        errno(ptrace(
            kernel,
            &child_caller,
            R::PTRACE_TRACEME,
            TaskId(0),
            0,
            0
        )),
        Errno::EPERM
    );
    assert!(waits_stopped(kernel, child));
    assert_eq!(wait(kernel, &caller, child, WALL), None, "reported once");

    // Discard the SIGSTOP; the child runs.
    assert_eq!(ptrace(kernel, &caller, R::PTRACE_CONT, child, 0, 0), Ok(0));
    assert!(!waits_stopped(kernel, child));
    assert_eq!(kernel.take_pending(child), None);
    assert_eq!(
        errno(ptrace(
            kernel,
            &caller,
            R::PTRACE_GETSIGINFO,
            child,
            0,
            0x600
        )),
        Errno::ESRCH
    );

    // An ignored signal stops a traced task too; the tracer swaps it for SIGUSR2's
    // replacement, SIGWINCH, which is ignored.
    kernel.kill(child, Signal::SIGURG).unwrap();
    assert_eq!(kernel.take_pending(child), Some(GuestAction::Stopped));
    let (code, status, pid) = wait(kernel, &caller, child, WALL).unwrap();
    assert_eq!(
        (code, status, pid),
        (SiCode::CLD_TRAPPED, Signal::SIGURG.0, child.0 as i32)
    );
    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_GETSIGINFO, child, 0, 0x600),
        Ok(0)
    );
    let info: Siginfo = block_on(read_value(&memory, GuestPtr::new(0x600))).unwrap();
    assert_eq!(info.si_signo, Signal::SIGURG.0);
    assert_eq!(
        ptrace(
            kernel,
            &caller,
            R::PTRACE_CONT,
            child,
            0,
            Signal::SIGWINCH.0 as u32
        ),
        Ok(0)
    );
    assert_eq!(kernel.take_pending(child), None);

    // A signal the tracer passes on is delivered: SIGTERM's default action kills.
    kernel.kill(child, Signal::SIGTERM).unwrap();
    assert_eq!(kernel.take_pending(child), Some(GuestAction::Stopped));
    assert!(wait(kernel, &caller, child, WALL).is_some());
    assert_eq!(
        errno(ptrace(kernel, &caller, R::PTRACE_CONT, child, 0, 65)),
        Errno::EIO
    );
    assert_eq!(
        ptrace(
            kernel,
            &caller,
            R::PTRACE_CONT,
            child,
            0,
            Signal::SIGTERM.0 as u32
        ),
        Ok(0)
    );
    assert!(matches!(
        kernel.take_pending(child),
        Some(GuestAction::Exit(_))
    ));
}

/// `PTRACE_ATTACH` stops the task with SIGSTOP; in its stop the tracer reads and writes its
/// memory (`PEEKDATA`, `POKEDATA`), and there are no registers or steps (`EIO`). `PTRACE_DETACH`
/// lets it go.
#[test]
fn attach_peeks_and_pokes_the_tracees_memory() {
    let (stack, parent, memory) = machine();
    let kernel = &stack.inner;
    let caller = Caller::new(parent, &memory);
    let child = fork_child(kernel, &caller);
    let shared = SyncMemory::new(2 * 65536, 0x100);
    kernel.attach_memory(child, shared.clone());
    shared.put(0x2000, &0x1234_5678u32.to_le_bytes());

    assert_eq!(
        errno(ptrace(kernel, &caller, R::PTRACE_ATTACH, parent, 0, 0)),
        Errno::EPERM
    );
    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_ATTACH, child, 0, 0),
        Ok(0)
    );
    assert_eq!(
        errno(ptrace(kernel, &caller, R::PTRACE_ATTACH, child, 0, 0)),
        Errno::EPERM
    );
    // Not stopped yet.
    assert_eq!(
        errno(ptrace(
            kernel,
            &caller,
            R::PTRACE_PEEKDATA,
            child,
            0x2000,
            0x600
        )),
        Errno::ESRCH
    );
    assert_eq!(kernel.take_pending(child), Some(GuestAction::Stopped));
    let stopped = wait(kernel, &caller, child, WALL).map(|(code, status, _)| (code, status));
    assert_eq!(stopped, Some((SiCode::CLD_TRAPPED, Signal::SIGSTOP.0)));

    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_PEEKDATA, child, 0x2000, 0x600),
        Ok(0)
    );
    assert_eq!(get(&memory.0, 0x600, 4), 0x1234_5678u32.to_le_bytes());
    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_PEEKTEXT, child, 0x2000, 0x600),
        Ok(0)
    );
    assert_eq!(
        ptrace(
            kernel,
            &caller,
            R::PTRACE_POKEDATA,
            child,
            0x2004,
            0xdead_beef
        ),
        Ok(0)
    );
    assert_eq!(shared.get(0x2004, 4), 0xdead_beefu32.to_le_bytes());
    // kernel/ptrace.c ptrace_access_vm: SUID_DUMP_ROOT permits a privileged core
    // dump, but only SUID_DUMP_USER permits an unprivileged tracer to read memory.
    let process = kernel.process(child);
    process.ident.lock_or_recover().dumpable = crate::ident::Dumpable::Root;
    assert_eq!(
        errno(ptrace(
            kernel,
            &caller,
            R::PTRACE_PEEKDATA,
            child,
            0x2000,
            0x600
        )),
        Errno::EIO
    );
    assert!(!kernel.ptrace_may_access(parent, child, crate::ptrace::MODE_ATTACH_REALCREDS));
    process.ident.lock_or_recover().dumpable = crate::ident::Dumpable::User;
    let end = 2 * 65536 - 2;
    assert_eq!(
        errno(ptrace(
            kernel,
            &caller,
            R::PTRACE_PEEKDATA,
            child,
            end,
            0x600
        )),
        Errno::EIO
    );
    assert_eq!(
        errno(ptrace(kernel, &caller, R::PTRACE_POKEDATA, child, 0x10, 1)),
        Errno::EIO,
        "the guard"
    );
    for request in [
        R::PTRACE_PEEKUSR,
        R::PTRACE_POKEUSR,
        R::PTRACE_SINGLESTEP,
        R::PTRACE_GETREGSET,
    ] {
        assert_eq!(
            errno(ptrace(kernel, &caller, request, child, 0, 0x600)),
            Errno::EIO,
            "{request:?}"
        );
    }

    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_DETACH, child, 0, 0),
        Ok(0)
    );
    assert_eq!(kernel.take_pending(child), None);
    assert_eq!(
        errno(ptrace(kernel, &caller, R::PTRACE_CONT, child, 0, 0)),
        Errno::ESRCH
    );
}

/// `PTRACE_SYSCALL` with `PTRACE_O_TRACESYSGOOD`: the call stops at its entry and at its exit
/// with `SIGTRAP | 0x80`, which libc's `waitpid` reports as `WSTOPSIG`, and
/// `PTRACE_GET_SYSCALL_INFO` tells the call's number and arguments, then its result.
#[test]
fn syscall_stops_report_entry_and_exit() {
    let (stack, parent, memory) = machine();
    let kernel = &stack.inner;
    let caller = Caller::new(parent, &memory);
    let child_memory = flat();
    let child = traced_child(kernel, &caller, &child_memory);
    let child_caller = Caller::new(child, &child_memory);
    let sysgood = PtraceOptions::PTRACE_O_TRACESYSGOOD.0;
    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_SETOPTIONS, child, 0, sysgood),
        Ok(0)
    );
    assert_eq!(
        errno(ptrace(
            kernel,
            &caller,
            R::PTRACE_SETOPTIONS,
            child,
            0,
            1 << 30
        )),
        Errno::EINVAL
    );
    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_SYSCALL, child, 0, 0),
        Ok(0)
    );
    assert_eq!(kernel.take_pending(child), None);

    let mut call = Box::pin(stack.getpid(&child_caller));
    assert!(
        call.as_mut().now_or_never().is_none(),
        "stopped at the entry"
    );
    let (code, status, _) = wait(kernel, &caller, child, WALL).unwrap();
    assert_eq!((code, status), (SiCode::CLD_TRAPPED, SIGTRAP | 0x80));
    assert_eq!(wait4_status(status), 0x857f);
    assert_eq!(
        ptrace(
            kernel,
            &caller,
            R::PTRACE_GET_SYSCALL_INFO,
            child,
            88,
            0x700
        ),
        Ok(80)
    );
    let info = get(&memory.0, 0x700, 88);
    assert_eq!(info[0], 1, "PTRACE_SYSCALL_INFO_ENTRY");
    assert_eq!(
        u32::from_le_bytes(info[4..8].try_into().unwrap()),
        0x4000_00f3,
        "AUDIT_ARCH_RISCV32"
    );
    assert_eq!(
        u64::from_le_bytes(info[24..32].try_into().unwrap()),
        NR_GETPID
    );
    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_GETEVENTMSG, child, 0, 0x680),
        Ok(0)
    );
    assert_eq!(
        get(&memory.0, 0x680, 4),
        1u32.to_le_bytes(),
        "PTRACE_EVENTMSG_SYSCALL_ENTRY"
    );
    // Only what fits is written; the whole size is returned.
    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_GET_SYSCALL_INFO, child, 8, 0x800),
        Ok(80)
    );

    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_SYSCALL, child, 0, 0),
        Ok(0)
    );
    assert!(
        call.as_mut().now_or_never().is_none(),
        "stopped at the exit"
    );
    let (_, status, _) = wait(kernel, &caller, child, WALL).unwrap();
    assert_eq!(status, SIGTRAP | 0x80);
    assert_eq!(
        ptrace(
            kernel,
            &caller,
            R::PTRACE_GET_SYSCALL_INFO,
            child,
            88,
            0x700
        ),
        Ok(33)
    );
    let info = get(&memory.0, 0x700, 33);
    assert_eq!(info[0], 2, "PTRACE_SYSCALL_INFO_EXIT");
    let pid = i64::from_le_bytes(info[24..32].try_into().unwrap());
    assert_eq!((pid, info[32]), (i64::from(child.0), 0));

    assert_eq!(ptrace(kernel, &caller, R::PTRACE_CONT, child, 0, 0), Ok(0));
    assert_eq!(call.as_mut().now_or_never(), Some(Ok(child.0 as i32)));
    // Without PTRACE_SYSCALL nothing stops.
    assert_eq!(block_on(stack.getpid(&child_caller)), Ok(child.0 as i32));
}

/// `PTRACE_SEIZE` does not stop the task; `PTRACE_INTERRUPT` makes it trap with
/// `PTRACE_EVENT_STOP`, whose event bits wait reports above SIGTRAP; `PTRACE_LISTEN` keeps it
/// there without a report.
#[test]
fn seize_interrupt_and_listen() {
    let (stack, parent, memory) = machine();
    let kernel = &stack.inner;
    let caller = Caller::new(parent, &memory);
    let child = fork_child(kernel, &caller);
    let options = PtraceOptions::PTRACE_O_TRACESYSGOOD.0;
    assert_eq!(
        errno(ptrace(kernel, &caller, R::PTRACE_SEIZE, child, 4, 0)),
        Errno::EIO
    );
    assert_eq!(
        errno(ptrace(kernel, &caller, R::PTRACE_SEIZE, child, 0, 1 << 30)),
        Errno::EIO
    );
    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_SEIZE, child, 0, options),
        Ok(0)
    );
    assert_eq!(kernel.take_pending(child), None, "no SIGSTOP");
    assert_eq!(wait(kernel, &caller, child, WALL), None);

    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_INTERRUPT, child, 0, 0),
        Ok(0)
    );
    assert_eq!(kernel.take_pending(child), Some(GuestAction::Stopped));
    let event_stop = SIGTRAP | PtraceEvent::PTRACE_EVENT_STOP.0 << 8;
    let (code, status, _) = wait(kernel, &caller, child, WALL).unwrap();
    // A tracer's wait reports every ptrace stop as trapped (`wait_task_stopped`).
    assert_eq!((code, status), (SiCode::CLD_TRAPPED, event_stop));
    assert_eq!(wait4_status(status) >> 16, PtraceEvent::PTRACE_EVENT_STOP.0);
    assert_eq!(wait4_status(status) >> 8 & 0xff, SIGTRAP);
    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_GETSIGINFO, child, 0, 0x600),
        Ok(0)
    );
    let info: Siginfo = block_on(read_value(&memory, GuestPtr::new(0x600))).unwrap();
    assert_eq!(info.si_code, event_stop);

    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_LISTEN, child, 0, 0),
        Ok(0)
    );
    assert!(waits_stopped(kernel, child));
    assert_eq!(
        errno(ptrace(kernel, &caller, R::PTRACE_CONT, child, 0, 0)),
        Errno::ESRCH,
        "listening"
    );
    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_INTERRUPT, child, 0, 0),
        Ok(0)
    );
    assert_eq!(kernel.take_pending(child), Some(GuestAction::Stopped));
    assert!(wait(kernel, &caller, child, WALL).is_some());
    assert_eq!(ptrace(kernel, &caller, R::PTRACE_CONT, child, 0, 0), Ok(0));
    assert_eq!(kernel.take_pending(child), None);
}

/// With `PTRACE_O_TRACEFORK` a fork stops the parent with `PTRACE_EVENT_FORK`, whose message
/// is the child's pid, and the child is traced too, starting in a trap stop.
#[test]
fn a_traced_fork_reports_the_child_which_is_traced_too() {
    let (stack, tracer, memory) = machine();
    let kernel = &stack.inner;
    let caller = Caller::new(tracer, &memory);
    let child = fork_child(kernel, &caller);
    let options = PtraceOptions::PTRACE_O_TRACEFORK.0;
    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_SEIZE, child, 0, options),
        Ok(0)
    );

    let child_memory = ForkableMemory(flat());
    let child_caller = Caller::new(child, &child_memory);
    let sigchld = u32::try_from(Signal::SIGCHLD.0).unwrap();
    let (parent_tid, child_tid) = (S32Ptr::new(8), S32Ptr::new(16));
    let mut fork = Box::pin(kernel.clone(&child_caller, sigchld, 0, parent_tid, 0, child_tid));
    assert!(fork.as_mut().now_or_never().is_none(), "the event stop");
    let (code, status, _) = wait(kernel, &caller, child, WALL).unwrap();
    assert_eq!(
        (code, status),
        (
            SiCode::CLD_TRAPPED,
            SIGTRAP | PtraceEvent::PTRACE_EVENT_FORK.0 << 8
        )
    );
    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_GETEVENTMSG, child, 0, 0x680),
        Ok(0)
    );
    let grandchild = TaskId(u32::from_le_bytes(
        get(&memory.0, 0x680, 4).try_into().unwrap(),
    ));
    assert_ne!(grandchild, child);

    assert_eq!(kernel.task_signals(grandchild).tracer(), Some(tracer));
    assert_eq!(kernel.take_pending(grandchild), Some(GuestAction::Stopped));
    let (_, status, pid) = wait(kernel, &caller, grandchild, WALL).unwrap();
    assert_eq!(status, SIGTRAP | PtraceEvent::PTRACE_EVENT_STOP.0 << 8);
    assert_eq!(pid, grandchild.0 as i32);

    assert_eq!(ptrace(kernel, &caller, R::PTRACE_CONT, child, 0, 0), Ok(0));
    assert_eq!(fork.as_mut().now_or_never(), Some(Ok(grandchild.0 as i32)));
}

/// A tracee's end is reported to a tracer that is not its parent (`WEXITED`), and a tracer's
/// end lets its tracees go, killing them with `PTRACE_O_EXITKILL`.
#[test]
fn the_ends_of_tracee_and_tracer() {
    let (stack, parent, memory) = machine();
    let kernel = &stack.inner;
    let caller = Caller::new(parent, &memory);
    // Siblings: Yama's default lets only an ancestor attach.
    let tracer = fork_child(kernel, &caller);
    let tracee = fork_child(kernel, &caller);
    let victim = fork_child(kernel, &caller);
    let tracer_memory = flat();
    let tracer_caller = Caller::new(tracer, &tracer_memory);
    assert_eq!(
        errno(ptrace(
            kernel,
            &tracer_caller,
            R::PTRACE_SEIZE,
            tracee,
            0,
            0
        )),
        Errno::EPERM
    );
    kernel.yama.set_scope(0).unwrap();
    assert_eq!(
        ptrace(kernel, &tracer_caller, R::PTRACE_SEIZE, tracee, 0, 0),
        Ok(0)
    );
    let exitkill = PtraceOptions::PTRACE_O_EXITKILL.0;
    assert_eq!(
        ptrace(kernel, &tracer_caller, R::PTRACE_SEIZE, victim, 0, exitkill),
        Ok(0)
    );

    let tracee_memory = flat();
    let tracee_caller = Caller::new(tracee, &tracee_memory);
    assert_eq!(block_on(kernel.exit_group(&tracee_caller, 3)), Ok(()));
    assert!(matches!(
        kernel.take_pending(tracee),
        Some(GuestAction::Exit(3))
    ));
    let reported = wait(kernel, &tracer_caller, tracee, WALL);
    assert_eq!(
        reported.map(|(code, status, _)| (code, status)),
        Some((SiCode::CLD_EXITED, 3))
    );
    // Its parent still reaps the zombie.
    let reaped = wait(kernel, &caller, tracee, 0).map(|(code, status, _)| (code, status));
    assert_eq!(reaped, Some((SiCode::CLD_EXITED, 3)));

    assert_eq!(block_on(kernel.exit_group(&tracer_caller, 0)), Ok(()));
    assert!(matches!(
        kernel.take_pending(tracer),
        Some(GuestAction::Exit(0))
    ));
    assert_eq!(kernel.task_signals(victim).tracer(), None);
    assert!(
        matches!(kernel.take_pending(victim), Some(GuestAction::Exit(_))),
        "SIGKILL"
    );
}

/// Yama's scope 3 forbids even `PTRACE_TRACEME`; scope 2 needs `CAP_SYS_PTRACE` of the parent.
#[test]
fn yama_scopes_limit_traceme() {
    let (stack, parent, memory) = machine();
    let kernel = &stack.inner;
    let caller = Caller::new(parent, &memory);
    let child = fork_child(kernel, &caller);
    let child_memory = flat();
    let child_caller = Caller::new(child, &child_memory);
    kernel.yama.set_scope(2).unwrap();
    assert_eq!(
        errno(ptrace(
            kernel,
            &child_caller,
            R::PTRACE_TRACEME,
            TaskId(0),
            0,
            0
        )),
        Errno::EPERM
    );
    kernel.yama.set_scope(3).unwrap();
    assert_eq!(
        errno(ptrace(
            kernel,
            &child_caller,
            R::PTRACE_TRACEME,
            TaskId(0),
            0,
            0
        )),
        Errno::EPERM
    );
}

/// `execve` of a task traced the old way, without `PTRACE_O_TRACEEXEC`, sends it SIGTRAP; with
/// the option it stops with `PTRACE_EVENT_EXEC`.
#[test]
fn execve_of_a_tracee_traps() {
    let (stack, parent, memory) = machine();
    let kernel = &stack.inner;
    let caller = Caller::new(parent, &memory);
    let child_memory = flat();
    let child = traced_child(kernel, &caller, &child_memory);
    let child_caller = Caller::new(child, &child_memory);
    super::task::install_prog(kernel, &child_caller, &child_memory);
    assert_eq!(ptrace(kernel, &caller, R::PTRACE_CONT, child, 0, 0), Ok(0));
    assert_eq!(kernel.take_pending(child), None);
    assert_eq!(block_on(super::task::execve(kernel, &child_caller)), Ok(0));
    while let Some(action) = kernel.take_pending(child) {
        if action == GuestAction::Stopped {
            break;
        }
    }
    let (code, status, _) = wait(kernel, &caller, child, WALL).unwrap();
    assert_eq!((code, status), (SiCode::CLD_TRAPPED, SIGTRAP));

    let exec = PtraceOptions::PTRACE_O_TRACEEXEC.0;
    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_SETOPTIONS, child, 0, exec),
        Ok(0)
    );
    assert_eq!(ptrace(kernel, &caller, R::PTRACE_CONT, child, 0, 0), Ok(0));
    while kernel.take_pending(child).is_some() {}
    let mut call = Box::pin(super::task::execve(kernel, &child_caller));
    assert!(call.as_mut().now_or_never().is_none());
    let (_, status, _) = wait(kernel, &caller, child, WALL).unwrap();
    assert_eq!(status, SIGTRAP | PtraceEvent::PTRACE_EVENT_EXEC.0 << 8);
    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_GETEVENTMSG, child, 0, 0x680),
        Ok(0)
    );
    assert_eq!(
        get(&memory.0, 0x680, 4),
        child.0.to_le_bytes(),
        "the old tid"
    );
    assert_eq!(ptrace(kernel, &caller, R::PTRACE_CONT, child, 0, 0), Ok(0));
    assert_eq!(call.as_mut().now_or_never(), Some(Ok(0)));
}

/// Writes the iovec `{base, len}` at `at`.
fn iovec(memory: &FlatMemory, at: u32, base: u32, len: u32) {
    put(memory, at, &base.to_le_bytes());
    put(memory, at + 4, &len.to_le_bytes());
}

/// `process_vm_readv` and `process_vm_writev` move bytes between the caller's memory and that
/// of a process it may attach to; a remote range that faults ends the copy, `EFAULT` when
/// nothing moved.
#[test]
fn process_vm_readv_and_writev_copy_between_processes() {
    let (stack, parent, memory) = machine();
    let kernel = &stack.inner;
    let caller = Caller::new(parent, &memory);
    let child = fork_child(kernel, &caller);
    let page = 65536;
    let shared = SyncMemory::new(2 * page as usize, 0x100);
    kernel.attach_memory(child, shared.clone());
    shared.put(0x3000, b"remote!!");
    let readv = |local: u32, n: u32, remote: u32, rn: u32, flags: u32| {
        block_on(kernel.process_vm_readv(
            &caller,
            child.0 as i32,
            GuestPtr::new(local),
            n,
            GuestPtr::new(remote),
            rn,
            flags,
        ))
    };

    iovec(&memory.0, 0x100, 0x1000, 8);
    iovec(&memory.0, 0x110, 0x3000, 8);
    assert_eq!(readv(0x100, 1, 0x110, 1, 0), Ok(8));
    assert_eq!(get(&memory.0, 0x1000, 8), b"remote!!");

    // Two local iovecs fed from one remote one.
    iovec(&memory.0, 0x100, 0x1100, 3);
    iovec(&memory.0, 0x108, 0x1200, 5);
    assert_eq!(readv(0x100, 2, 0x110, 1, 0), Ok(8));
    assert_eq!(
        (get(&memory.0, 0x1100, 3), get(&memory.0, 0x1200, 5)),
        (b"rem".to_vec(), b"ote!!".to_vec())
    );

    // The remote range runs off the end of its memory: what came before is returned.
    iovec(&memory.0, 0x100, 0x1000, 16);
    iovec(&memory.0, 0x110, 2 * page - 4, 16);
    assert_eq!(readv(0x100, 1, 0x110, 1, 0), Ok(4));
    iovec(&memory.0, 0x110, 3 * page, 16);
    assert_eq!(errno(readv(0x100, 1, 0x110, 1, 0)), Errno::EFAULT);
    assert_eq!(errno(readv(0x100, 1, 0x110, 1, 1)), Errno::EINVAL);
    assert_eq!(readv(0x100, 0, 0x110, 1, 0), Ok(0));

    put(&memory.0, 0x1000, b"written!");
    iovec(&memory.0, 0x100, 0x1000, 8);
    iovec(&memory.0, 0x110, 0x4000, 8);
    let writev = kernel.process_vm_writev(
        &caller,
        child.0 as i32,
        GuestPtr::new(0x100),
        1,
        GuestPtr::new(0x110),
        1,
        0,
    );
    assert_eq!(block_on(writev), Ok(8));
    assert_eq!(shared.get(0x4000, 8), b"written!");

    // A sibling may not attach under Yama's default.
    let sibling = fork_child(kernel, &caller);
    let sibling_memory = flat();
    let sibling_caller = Caller::new(sibling, &sibling_memory);
    iovec(&sibling_memory, 0x100, 0x1000, 8);
    iovec(&sibling_memory, 0x110, 0x3000, 8);
    let theirs = kernel.process_vm_readv(
        &sibling_caller,
        child.0 as i32,
        GuestPtr::new(0x100),
        1,
        GuestPtr::new(0x110),
        1,
        0,
    );
    assert_eq!(errno(block_on(theirs)), Errno::EPERM);
    let nobody = kernel.process_vm_readv(
        &caller,
        4242,
        GuestPtr::new(0x100),
        1,
        GuestPtr::new(0x110),
        1,
        0,
    );
    assert_eq!(errno(block_on(nobody)), Errno::ESRCH);
}

/// `/proc/<pid>/mem` reads and writes the address space at the file offset, for an opener that
/// may attach (`EACCES` otherwise); an address nothing backs is `EIO`.
#[test]
fn proc_pid_mem_reads_and_writes_the_address_space() {
    let (stack, parent, memory) = machine();
    let kernel = &stack.inner;
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(
        b"/proc",
        kernel.procfs(),
        wasixv2_vfs::MountFlags::default(),
        true,
    ))
    .unwrap();
    let caller = Caller::new(parent, &memory);
    let child = fork_child(kernel, &caller);
    let shared = SyncMemory::new(2 * 65536, 0x100);
    kernel.attach_memory(child, shared.clone());
    shared.put(0x3000, b"in child");

    let open = |caller: &Caller<'_>, memory: &FlatMemory| {
        put(
            memory,
            0x2000,
            format!("/proc/{}/mem\0", child.0).as_bytes(),
        );
        block_on(kernel.openat(
            caller,
            Fd(-100),
            GuestPtr::new(0x2000),
            OpenFlags::O_RDWR,
            0,
        ))
    };
    let fd = open(&caller, &memory.0).unwrap();
    let read = block_on(kernel.pread64(&caller, fd, UserSliceMut::new(0x1000, 8), 0x3000));
    assert_eq!(read, Ok(8));
    assert_eq!(get(&memory.0, 0x1000, 8), b"in child");
    put(&memory.0, 0x1000, b"from mem");
    let wrote = block_on(kernel.pwrite64(&caller, fd, UserSlice::new(0x1000, 8), 0x4000));
    assert_eq!(wrote, Ok(8));
    assert_eq!(shared.get(0x4000, 8), b"from mem");
    let past = block_on(kernel.pread64(&caller, fd, UserSliceMut::new(0x1000, 8), 3 * 65536));
    assert_eq!(errno(past), Errno::EIO);

    let sibling = fork_child(kernel, &caller);
    let sibling_memory = flat();
    let sibling_caller = Caller::new(sibling, &sibling_memory);
    assert_eq!(errno(open(&sibling_caller, &sibling_memory)), Errno::EACCES);
}

/// kernel/exit.c `do_exit` never reaches syscall-exit tracing. The layer may see
/// an unparented tracer already reaped when `exit_group` returns internally.
#[test]
fn syscall_exit_does_not_inspect_a_reaped_tracer() {
    let (stack, tracer, memory) = machine();
    let caller = Caller::new(tracer, &memory);
    let child = fork_child(&stack.inner, &caller);
    assert_eq!(
        ptrace(&stack.inner, &caller, R::PTRACE_SEIZE, child, 0, 0),
        Ok(0)
    );
    assert_eq!(block_on(stack.exit_group(&caller, 0)), Ok(()));
    assert!(stack.inner.task_process(tracer).is_none());
}

/// kernel/fork.c `kernel_clone` reports VFORK_DONE only after the child leaves its mm.
#[test]
fn vfork_done_reports_the_child_before_the_parent_resumes() {
    for shared in [false, true] {
        let (stack, tracer, memory) = machine();
        let kernel = &stack.inner;
        let caller = Caller::new(tracer, &memory);
        let parent = fork_child(kernel, &caller);
        assert_eq!(
            ptrace(
                kernel,
                &caller,
                R::PTRACE_SEIZE,
                parent,
                0,
                PtraceOptions::PTRACE_O_TRACEVFORKDONE.0
            ),
            Ok(0)
        );
        let parent_memory = ForkableMemory(flat());
        let parent_caller = Caller::new(parent, &parent_memory);
        let flags = 0x4000 | if shared { 0x100 } else { 0 } | Signal::SIGCHLD.0 as u32;
        let result =
            block_on(kernel.clone(&parent_caller, flags, 0, S32Ptr::NULL, 0, S32Ptr::NULL))
                .unwrap();
        let child = match kernel.take_pending(parent).unwrap() {
            GuestAction::SwitchTask { task, .. } => task,
            GuestAction::Fork { child, .. } => child,
            other => panic!("unexpected action: {other:?}"),
        };
        if !shared {
            assert_eq!(result, child.0 as i32);
        }
        assert_eq!(wait(kernel, &caller, parent, WALL), None);
        let child_memory = flat();
        block_on(kernel.exit_group(&Caller::new(child, &child_memory), 0)).unwrap();
        assert_eq!(kernel.take_pending(parent), Some(GuestAction::Stopped));
        assert_eq!(
            wait(kernel, &caller, parent, WALL).unwrap().1,
            SIGTRAP | PtraceEvent::PTRACE_EVENT_VFORK_DONE.0 << 8
        );
        assert_eq!(
            ptrace(kernel, &caller, R::PTRACE_GETEVENTMSG, parent, 0, 0x680),
            Ok(0)
        );
        assert_eq!(get(&memory.0, 0x680, 4), child.0.to_le_bytes());
        assert_eq!(ptrace(kernel, &caller, R::PTRACE_CONT, parent, 0, 0), Ok(0));
        // The child's SIGCHLD is delivered after the vfork completion event.
        assert_eq!(kernel.take_pending(parent), Some(GuestAction::Stopped));
        assert_eq!(
            wait(kernel, &caller, parent, WALL).unwrap().1,
            Signal::SIGCHLD.0
        );
        assert_eq!(ptrace(kernel, &caller, R::PTRACE_CONT, parent, 0, 0), Ok(0));
        assert_eq!(kernel.take_pending(parent), None);
    }
}

/// kernel/fork.c `kernel_clone` reports VFORK_DONE only after the child leaves its mm.
#[test]
fn vfork_done_keeps_the_child_pid_after_autoreap() {
    for shared in [false, true] {
        let (stack, tracer, memory) = machine();
        let kernel = &stack.inner;
        let caller = Caller::new(tracer, &memory);
        let parent = fork_child(kernel, &caller);
        assert_eq!(
            ptrace(
                kernel,
                &caller,
                R::PTRACE_SEIZE,
                parent,
                0,
                PtraceOptions::PTRACE_O_TRACEVFORKDONE.0
            ),
            Ok(0)
        );
        let parent_memory = ForkableMemory(flat());
        let parent_caller = Caller::new(parent, &parent_memory);
        let action = wasixv2_abi::Sigaction {
            sa_handler: wasixv2_abi::SignalHandler::SIG_IGN.0 as u32,
            ..Default::default()
        };
        block_on(wasixv2_syscalls::write_value(
            &parent_memory,
            GuestPtrMut::new(0x900),
            &action,
        ))
        .unwrap();
        block_on(kernel.rt_sigaction(
            &parent_caller,
            Signal::SIGCHLD,
            GuestPtr::new(0x900),
            GuestPtrMut::NULL,
            8,
        ))
        .unwrap();
        let flags = 0x4000 | if shared { 0x100 } else { 0 } | Signal::SIGCHLD.0 as u32;
        let result =
            block_on(kernel.clone(&parent_caller, flags, 0, S32Ptr::NULL, 0, S32Ptr::NULL))
                .unwrap();
        let child = match kernel.take_pending(parent).unwrap() {
            GuestAction::SwitchTask { task, .. } => task,
            GuestAction::Fork { child, .. } => child,
            other => panic!("unexpected action: {other:?}"),
        };
        if !shared {
            assert_eq!(result, child.0 as i32);
        }
        assert_eq!(wait(kernel, &caller, parent, WALL), None);
        let child_memory = flat();
        block_on(kernel.exit_group(&Caller::new(child, &child_memory), 0)).unwrap();
        // kernel_clone retains struct pid even if ignored SIGCHLD autoreaps the child.
        block_on(kernel.exit_mm(&Caller::new(child, &child_memory)));
        kernel.release(child);
        assert!(kernel.task_process(child).is_none());
        assert_eq!(kernel.take_pending(parent), Some(GuestAction::Stopped));
        assert_eq!(
            wait(kernel, &caller, parent, WALL).unwrap().1,
            SIGTRAP | PtraceEvent::PTRACE_EVENT_VFORK_DONE.0 << 8
        );
        assert_eq!(
            ptrace(kernel, &caller, R::PTRACE_GETEVENTMSG, parent, 0, 0x680),
            Ok(0)
        );
        assert_eq!(get(&memory.0, 0x680, 4), child.0.to_le_bytes());
        assert_eq!(ptrace(kernel, &caller, R::PTRACE_CONT, parent, 0, 0), Ok(0));
        assert_eq!(kernel.take_pending(parent), None);
    }
}

/// kernel/exit.c `do_exit` reports EXIT for a fatal signal, including SIGKILL.
#[test]
fn fatal_signal_reports_exit_before_teardown() {
    for sig in [Signal::SIGKILL, Signal::SIGTERM] {
        let (stack, tracer, memory) = machine();
        let kernel = &stack.inner;
        let caller = Caller::new(tracer, &memory);
        let child = fork_child(kernel, &caller);
        assert_eq!(
            ptrace(
                kernel,
                &caller,
                R::PTRACE_SEIZE,
                child,
                0,
                PtraceOptions::PTRACE_O_TRACEEXIT.0
            ),
            Ok(0)
        );
        kernel.kill(child, sig).unwrap();
        assert_eq!(kernel.take_pending(child), Some(GuestAction::Stopped));
        if sig != Signal::SIGKILL {
            assert_eq!(wait(kernel, &caller, child, WALL).unwrap().1, sig.0);
            assert_eq!(
                ptrace(kernel, &caller, R::PTRACE_CONT, child, 0, sig.0 as u32),
                Ok(0)
            );
            assert_eq!(kernel.take_pending(child), Some(GuestAction::Stopped));
        }
        assert_eq!(
            wait(kernel, &caller, child, WALL).unwrap().1,
            SIGTRAP | PtraceEvent::PTRACE_EVENT_EXIT.0 << 8
        );
        assert_eq!(
            ptrace(kernel, &caller, R::PTRACE_GETEVENTMSG, child, 0, 0x680),
            Ok(0)
        );
        assert_eq!(get(&memory.0, 0x680, 4), (sig.0 as u32).to_le_bytes());
        assert_eq!(kernel.take_pending(child), Some(GuestAction::Stopped));
        assert!(waits_stopped(kernel, child));
        assert_eq!(ptrace(kernel, &caller, R::PTRACE_CONT, child, 0, 0), Ok(0));
        assert_eq!(
            kernel.take_pending(child),
            Some(GuestAction::Exit(128 + sig.0))
        );
        assert_eq!(kernel.take_pending(child), None);
    }
}

/// kernel/exit.c `wait_task_zombie`: a non-parent tracer consumes EXIT_TRACE first.
#[test]
fn tracer_reaps_before_the_real_parent() {
    for tracer_exits in [false, true] {
        let (stack, parent, memory) = machine();
        let kernel = &stack.inner;
        let caller = Caller::new(parent, &memory);
        kernel.yama.set_scope(0).unwrap();
        let tracer = fork_child(kernel, &caller);
        let child = fork_child(kernel, &caller);
        let tracer_memory = flat();
        let tracer_caller = Caller::new(tracer, &tracer_memory);
        assert_eq!(
            ptrace(kernel, &tracer_caller, R::PTRACE_SEIZE, child, 0, 0),
            Ok(0)
        );
        let child_memory = flat();
        block_on(kernel.exit_group(&Caller::new(child, &child_memory), 7)).unwrap();
        assert_eq!(kernel.take_pending(child), Some(GuestAction::Exit(7)));
        assert_eq!(
            wait(kernel, &caller, child, 0),
            None,
            "hidden zombie still counts as a child"
        );
        if tracer_exits {
            block_on(kernel.exit_group(&tracer_caller, 0)).unwrap();
            assert_eq!(kernel.take_pending(tracer), Some(GuestAction::Exit(0)));
        } else {
            assert_eq!(
                wait(kernel, &tracer_caller, child, WALL | WaitOptions::WNOWAIT.0)
                    .unwrap()
                    .1,
                7
            );
            assert_eq!(wait(kernel, &caller, child, 0), None);
            assert_eq!(wait(kernel, &tracer_caller, child, WALL).unwrap().1, 7);
        }
        assert_eq!(wait(kernel, &caller, child, 0).unwrap().1, 7);
    }
}

/// kernel/exit.c `exit_notify`: tracing prevents autoreap even with SIGCHLD ignored.
#[test]
fn a_parent_tracer_can_reap_with_sigchld_ignored() {
    let (stack, parent, memory) = machine();
    let kernel = &stack.inner;
    let caller = Caller::new(parent, &memory);
    let child = fork_child(kernel, &caller);
    assert_eq!(ptrace(kernel, &caller, R::PTRACE_SEIZE, child, 0, 0), Ok(0));
    let action = wasixv2_abi::Sigaction {
        sa_handler: wasixv2_abi::SignalHandler::SIG_IGN.0 as u32,
        ..Default::default()
    };
    block_on(wasixv2_syscalls::write_value(
        &memory,
        GuestPtrMut::new(0x900),
        &action,
    ))
    .unwrap();
    block_on(kernel.rt_sigaction(
        &caller,
        Signal::SIGCHLD,
        GuestPtr::new(0x900),
        GuestPtrMut::NULL,
        8,
    ))
    .unwrap();
    let child_memory = flat();
    block_on(kernel.exit_group(&Caller::new(child, &child_memory), 5)).unwrap();
    assert_eq!(kernel.take_pending(child), Some(GuestAction::Exit(5)));
    assert_eq!(wait(kernel, &caller, child, WALL).unwrap().1, 5);
}

/// fs/exec.c `de_thread`/`exec_binprm`: a non-leader keeps its tracer and reports its old tid
/// under the leader's pid, with no phantom exit for the old leader or caller.
#[test]
fn non_leader_exec_moves_the_trace_link_and_reports_old_tid() {
    let (stack, tracer, memory) = machine();
    let kernel = &stack.inner;
    let caller = Caller::new(tracer, &memory);
    let leader = fork_child(kernel, &caller);
    let child_memory = flat();
    let leader_caller = Caller::new(leader, &child_memory);
    super::task::install_prog(kernel, &leader_caller, &child_memory);
    let tid =
        block_on(kernel.thread_spawn(&leader_caller, 0x10f00, 0, S32Ptr::NULL, 0, S32Ptr::NULL))
            .unwrap();
    let thread = TaskId(tid as u32);
    while kernel.take_pending(leader).is_some() {}
    for task in [leader, thread] {
        assert_eq!(
            ptrace(
                kernel,
                &caller,
                R::PTRACE_SEIZE,
                task,
                0,
                PtraceOptions::PTRACE_O_TRACEEXEC.0
            ),
            Ok(0)
        );
    }
    let thread_caller = Caller::new(thread, &child_memory);
    let mut exec = Box::pin(super::task::execve(kernel, &thread_caller));
    assert!(exec.as_mut().now_or_never().is_none());
    assert!(matches!(
        kernel.take_pending(leader),
        Some(GuestAction::Exec { .. })
    ));
    let mut start = Box::pin(kernel.exec_wait(leader));
    assert!(start.as_mut().now_or_never().is_none());
    assert_eq!(exec.as_mut().now_or_never(), Some(Ok(0)));
    assert_eq!(kernel.take_pending(thread), Some(GuestAction::Exit(0)));
    kernel.release(thread);
    assert!(start.as_mut().now_or_never().is_none(), "exec event stop");
    assert_eq!(
        wait(kernel, &caller, leader, WALL).unwrap().1,
        SIGTRAP | PtraceEvent::PTRACE_EVENT_EXEC.0 << 8
    );
    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_GETEVENTMSG, leader, 0, 0x680),
        Ok(0)
    );
    assert_eq!(get(&memory.0, 0x680, 4), thread.0.to_le_bytes());
    assert_eq!(ptrace(kernel, &caller, R::PTRACE_CONT, leader, 0, 0), Ok(0));
    assert_eq!(start.as_mut().now_or_never(), Some(Ok(())));
    assert_eq!(kernel.task_signals(leader).tracer(), Some(tracer));
    assert!(!kernel.tracees.lock().contains_key(&thread));
}

fn syscall_info(memory: &FlatMemory, op: u8, value: u64, args: [u64; 6]) {
    let mut info = [0; 88];
    info[0] = op;
    info[24..32].copy_from_slice(&value.to_le_bytes());
    for (i, arg) in args.into_iter().enumerate() {
        info[32 + i * 8..40 + i * 8].copy_from_slice(&arg.to_le_bytes());
    }
    put(memory, 0x700, &info);
}

/// kernel/ptrace.c `ptrace_set_syscall_info`: change nr/arguments at entry, rval at exit,
/// and skip with -1. Typed import return variants must survive a different syscall.
#[test]
fn set_syscall_info_edits_entry_exit_and_skips_calls() {
    let (stack, tracer, memory) = machine();
    let kernel = &stack.inner;
    let caller = Caller::new(tracer, &memory);
    let child_memory = flat();
    let child = traced_child(kernel, &caller, &child_memory);
    let child_caller = Caller::new(child, &child_memory);
    assert_eq!(
        ptrace(
            kernel,
            &caller,
            R::PTRACE_SETOPTIONS,
            child,
            0,
            PtraceOptions::PTRACE_O_TRACESYSGOOD.0
        ),
        Ok(0)
    );
    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_SYSCALL, child, 0, 0),
        Ok(0)
    );
    assert_eq!(kernel.take_pending(child), None);
    let mut call = Box::pin(stack.getpid(&child_caller));
    assert!(call.as_mut().now_or_never().is_none());
    let set = |size| {
        ptrace(
            kernel,
            &caller,
            R::PTRACE_SET_SYSCALL_INFO,
            child,
            size,
            0x700,
        )
    };
    syscall_info(&memory.0, 1, 173, [0; 6]); // getppid
    assert_eq!(errno(set(87)), Errno::EINVAL);
    put(&memory.0, 0x701, &[1]);
    assert_eq!(errno(set(88)), Errno::EINVAL);
    syscall_info(&memory.0, 2, 0, [0; 6]);
    assert_eq!(errno(set(88)), Errno::EINVAL);
    syscall_info(&memory.0, 1, 1 << 32, [0; 6]);
    assert_eq!(errno(set(88)), Errno::ERANGE);
    syscall_info(&memory.0, 1, 173, [1 << 32; 6]);
    assert_eq!(errno(set(88)), Errno::ERANGE);
    syscall_info(&memory.0, 1, 173, [0; 6]);
    assert_eq!(set(88), Ok(0));
    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_SYSCALL, child, 0, 0),
        Ok(0)
    );
    assert!(call.as_mut().now_or_never().is_none());
    assert_eq!(
        ptrace(
            kernel,
            &caller,
            R::PTRACE_GET_SYSCALL_INFO,
            child,
            88,
            0x700
        ),
        Ok(33)
    );
    assert_eq!(get(&memory.0, 0x718, 8), u64::from(tracer.0).to_le_bytes());
    syscall_info(&memory.0, 2, (-(Errno::EPERM.0 as i64)) as u64, [0; 6]);
    assert_eq!(set(88), Ok(0));
    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_SYSCALL, child, 0, 0),
        Ok(0)
    );
    assert_eq!(call.as_mut().now_or_never(), Some(Err(Errno::EPERM.into())));
    drop(call);
    let mut call = Box::pin(stack.getpid(&child_caller));
    assert!(call.as_mut().now_or_never().is_none());
    syscall_info(&memory.0, 1, u64::MAX, [7; 6]);
    assert_eq!(set(88), Ok(0));
    assert_eq!(
        ptrace(
            kernel,
            &caller,
            R::PTRACE_GET_SYSCALL_INFO,
            child,
            88,
            0x700
        ),
        Ok(80)
    );
    assert_eq!(get(&memory.0, 0x718, 8), u64::MAX.to_le_bytes());
    assert_eq!(
        get(&memory.0, 0x720, 8),
        0u64.to_le_bytes(),
        "-1 keeps arguments"
    );
    assert_eq!(ptrace(kernel, &caller, R::PTRACE_CONT, child, 0, 0), Ok(0));
    assert_eq!(
        call.as_mut().now_or_never(),
        Some(Err(Errno::ENOSYS.into()))
    );
}

/// No registered rseq exists while sys_rseq returns ENOSYS. Linux copies only the prefix.
#[test]
fn get_rseq_configuration_reports_no_registration() {
    let (stack, tracer, memory) = machine();
    let kernel = &stack.inner;
    let caller = Caller::new(tracer, &memory);
    let child_memory = flat();
    let child = traced_child(kernel, &caller, &child_memory);
    put(&memory.0, 0x700, &[0xaa; 32]);
    assert_eq!(
        ptrace(
            kernel,
            &caller,
            R::PTRACE_GET_RSEQ_CONFIGURATION,
            child,
            9,
            0x700
        ),
        Ok(24)
    );
    assert_eq!(get(&memory.0, 0x700, 10), [vec![0; 9], vec![0xaa]].concat());
    assert_eq!(
        ptrace(
            kernel,
            &caller,
            R::PTRACE_GET_RSEQ_CONFIGURATION,
            child,
            32,
            0x700
        ),
        Ok(24)
    );
    assert_eq!(get(&memory.0, 0x700, 24), vec![0; 24]);
}

/// `forget_original_parent` calls exit_ptrace before autoreap can unhash the tracer.
#[test]
fn an_autoreaped_tracer_releases_its_zombie_tracees() {
    let (stack, parent, memory) = machine();
    let kernel = &stack.inner;
    let caller = Caller::new(parent, &memory);
    kernel.yama.set_scope(0).unwrap();
    let tracer = fork_child(kernel, &caller);
    let tracee = fork_child(kernel, &caller);
    let tracer_memory = flat();
    let tracer_caller = Caller::new(tracer, &tracer_memory);
    assert_eq!(
        ptrace(kernel, &tracer_caller, R::PTRACE_SEIZE, tracee, 0, 0),
        Ok(0)
    );
    let action = wasixv2_abi::Sigaction {
        sa_handler: wasixv2_abi::SignalHandler::SIG_IGN.0 as u32,
        ..Default::default()
    };
    block_on(wasixv2_syscalls::write_value(
        &memory,
        GuestPtrMut::new(0x900),
        &action,
    ))
    .unwrap();
    block_on(kernel.rt_sigaction(
        &caller,
        Signal::SIGCHLD,
        GuestPtr::new(0x900),
        GuestPtrMut::NULL,
        8,
    ))
    .unwrap();
    let tracee_memory = flat();
    block_on(kernel.exit_group(&Caller::new(tracee, &tracee_memory), 0)).unwrap();
    assert!(
        kernel.procs.find(tracee.0).is_some(),
        "tracer owns the zombie"
    );
    block_on(kernel.exit_group(&tracer_caller, 0)).unwrap();
    assert!(kernel.procs.find(tracer.0).is_none());
    assert!(
        kernel.procs.find(tracee.0).is_none(),
        "real parent autoreaps after detach"
    );
    assert!(!kernel.tracees.lock().contains_key(&tracee));
}

/// kernel/exit.c `do_exit` stops a killed vfork child before mm_release resumes its parent.
#[test]
fn a_vfork_childs_fatal_exit_stops_before_handing_back_the_instance() {
    let (stack, tracer, memory) = machine();
    let kernel = &stack.inner;
    let caller = Caller::new(tracer, &memory);
    let parent = fork_child(kernel, &caller);
    let parent_memory = flat();
    let parent_caller = Caller::new(parent, &parent_memory);
    assert_eq!(
        block_on(kernel.clone(
            &parent_caller,
            0x4100 | Signal::SIGCHLD.0 as u32,
            0,
            S32Ptr::NULL,
            0,
            S32Ptr::NULL
        )),
        Ok(0)
    );
    let Some(GuestAction::SwitchTask { task: child, .. }) = kernel.take_pending(parent) else {
        panic!("vfork did not switch tasks");
    };
    assert_eq!(
        ptrace(
            kernel,
            &caller,
            R::PTRACE_SEIZE,
            child,
            0,
            PtraceOptions::PTRACE_O_TRACEEXIT.0
        ),
        Ok(0)
    );
    kernel.kill(child, Signal::SIGKILL).unwrap();
    assert_eq!(kernel.take_pending(child), Some(GuestAction::Stopped));
    assert_eq!(
        wait(kernel, &caller, child, WALL).unwrap().1,
        SIGTRAP | PtraceEvent::PTRACE_EVENT_EXIT.0 << 8
    );
    assert_eq!(ptrace(kernel, &caller, R::PTRACE_CONT, child, 0, 0), Ok(0));
    assert!(matches!(
        kernel.take_pending(child),
        Some(GuestAction::VforkReturn { .. })
    ));
}

/// kernel/exit.c `delay_group_leader`: the tracer hears the final group exit even when the
/// execution layer released the leader's instance while another thread was still unwinding.
#[test]
fn a_released_leader_still_notifies_its_tracer_at_group_exit() {
    let (stack, tracer, memory) = machine();
    let kernel = &stack.inner;
    let caller = Caller::new(tracer, &memory);
    let leader = fork_child(kernel, &caller);
    let child_memory = flat();
    let leader_caller = Caller::new(leader, &child_memory);
    let thread = TaskId(
        block_on(kernel.thread_spawn(&leader_caller, 0x10f00, 0, S32Ptr::NULL, 0, S32Ptr::NULL))
            .unwrap() as u32,
    );
    while kernel.take_pending(leader).is_some() {}
    kernel
        .task_signals(tracer)
        .replace_blocked(1u64 << (Signal::SIGCHLD.0 - 1));
    assert_eq!(
        ptrace(kernel, &caller, R::PTRACE_SEIZE, leader, 0, 0),
        Ok(0)
    );
    block_on(kernel.exit_group(&leader_caller, 3)).unwrap();
    assert_eq!(kernel.take_pending(leader), Some(GuestAction::Exit(3)));
    kernel.release(leader);
    assert!(
        !kernel
            .group_signals(tracer.0)
            .unwrap()
            .is_pending(Signal::SIGCHLD.0)
    );
    assert_eq!(kernel.take_pending(thread), Some(GuestAction::Exit(3)));
    block_on(kernel.exit_mm(&Caller::new(thread, &child_memory)));
    kernel.release(thread);
    assert!(
        kernel
            .group_signals(tracer.0)
            .unwrap()
            .is_pending(Signal::SIGCHLD.0)
    );
    assert_eq!(wait(kernel, &caller, leader, WALL).unwrap().1, 3);
}
