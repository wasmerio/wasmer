//! Job control: group stops and SIGCONT, how a parent hears of them, and orphaned process
//! groups.

use futures::FutureExt as _;
use wasixv2_abi::{
    Idtype, OpenFlags, Pid, RusagePtr, SaFlags, SiCode, Sigaction, Siginfo, SiginfoPtr, Signal,
    Sigset, WaitOptions,
};
use wasixv2_syscalls::{read_value, write_value};
use wasixv2_wait::Interrupt as _;

use super::{
    ipc::{machine, pipe},
    proc::ForkableMemory,
    task::{execve, handle, install_prog},
    *,
};

/// The flags `pthread_create` passes.
const THREAD: u32 = 0x10f00;

fn spawn_thread(kernel: &Kernel, caller: &Caller<'_>) -> TaskId {
    let spawn = kernel.thread_spawn(caller, THREAD, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL);
    let tid = block_on(spawn).unwrap();
    while kernel.take_pending(caller.task()).is_some() {}
    TaskId(tid as u32)
}

/// Whether `task` waits in its stop: its stop wait does not end yet.
fn waits_stopped(kernel: &Kernel, task: TaskId) -> bool {
    Box::pin(kernel.stop_wait(task)).now_or_never().is_none()
}

/// The group of `task` stopped: every task took its part (`SIGNAL_STOP_STOPPED`).
fn group_stopped(kernel: &Kernel, task: TaskId) -> bool {
    kernel.task_signals(task).group().is_stopped()
}

/// `do_signal_stop`: SIGSTOP stops every thread of the group, each on its way back to the
/// guest, and the group is stopped once the last one took its part. SIGCONT wakes them all.
#[test]
fn sigstop_stops_every_thread_until_sigcont() {
    let (kernel, leader, memory) = machine();
    let caller = Caller::new(leader, &memory);
    let thread = spawn_thread(&kernel, &caller);

    kernel.kill(leader, Signal::SIGSTOP).unwrap();

    assert_eq!(kernel.take_pending(leader), Some(GuestAction::Stopped));
    assert!(waits_stopped(&kernel, leader));
    assert!(
        !group_stopped(&kernel, leader),
        "the thread has yet to stop"
    );
    assert!(
        kernel.task_signals(thread).pending(),
        "the thread leaves its wait"
    );
    assert!(kernel.work_flag(thread).is_raised());
    assert_eq!(kernel.take_pending(thread), Some(GuestAction::Stopped));
    assert!(waits_stopped(&kernel, thread));
    assert!(group_stopped(&kernel, leader));

    kernel.kill(leader, Signal::SIGCONT).unwrap();

    assert!(!waits_stopped(&kernel, leader));
    assert!(!waits_stopped(&kernel, thread));
    assert!(!group_stopped(&kernel, leader));
    assert_eq!(kernel.take_pending(leader), None);
    assert_eq!(kernel.take_pending(thread), None);
}

/// SIGCONT continues a stopped group whatever its disposition (`prepare_signal`), and a stop
/// signal pending when it comes is discarded.
#[test]
fn an_ignored_sigcont_continues_and_discards_pending_stops() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    super::signal::ignore(&kernel, &caller, Signal::SIGCONT);
    kernel.kill(task, Signal::SIGSTOP).unwrap();
    assert_eq!(kernel.take_pending(task), Some(GuestAction::Stopped));

    kernel.kill(task, Signal::SIGCONT).unwrap();
    assert!(!waits_stopped(&kernel, task));

    // A stop and a SIGCONT both pending: the SIGCONT took the stop away.
    kernel
        .task_signals(task)
        .replace_blocked(1 << (Signal::SIGTSTP.0 - 1));
    kernel.kill(task, Signal::SIGTSTP).unwrap();
    kernel.kill(task, Signal::SIGCONT).unwrap();
    kernel.task_signals(task).replace_blocked(0);
    assert_eq!(kernel.take_pending(task), None);
}

/// A blocking call a stop interrupts gives up (`ERESTARTSYS`), and with no handler runs again
/// once the group goes on.
#[test]
fn a_blocked_read_leaves_for_a_stop_and_restarts() {
    let (kernel, leader, memory) = machine();
    let caller = Caller::new(leader, &memory);
    let (r, _w) = pipe(&kernel, &caller, OpenFlags(0));
    let thread = spawn_thread(&kernel, &caller);
    let reader = Caller::new(thread, &memory);
    let mut read = Box::pin(kernel.read(&reader, r, UserSliceMut::new(0x300, 4)));
    assert_eq!(read.as_mut().now_or_never(), None);

    kernel.kill(leader, Signal::SIGSTOP).unwrap();
    assert_eq!(kernel.take_pending(leader), Some(GuestAction::Stopped));

    assert_eq!(read.as_mut().now_or_never(), Some(Err(SysErr::Restart)));
    assert_eq!(kernel.take_pending(thread), Some(GuestAction::Stopped));
    kernel.kill(leader, Signal::SIGCONT).unwrap();
    assert_eq!(
        kernel.take_pending(thread),
        None,
        "no handler: the read reruns"
    );
}

/// SIGKILL wakes a stopped group, which dies of it.
#[test]
fn sigkill_ends_a_stopped_group() {
    let (kernel, leader, memory) = machine();
    let caller = Caller::new(leader, &memory);
    let thread = spawn_thread(&kernel, &caller);
    kernel.kill(leader, Signal::SIGSTOP).unwrap();
    assert_eq!(kernel.take_pending(leader), Some(GuestAction::Stopped));
    assert_eq!(kernel.take_pending(thread), Some(GuestAction::Stopped));

    kernel.kill(leader, Signal::SIGKILL).unwrap();

    assert!(!waits_stopped(&kernel, leader));
    assert!(!waits_stopped(&kernel, thread));
    assert_eq!(
        kernel.take_pending(leader),
        Some(GuestAction::Exit(128 + 9))
    );
    assert_eq!(
        kernel.take_pending(thread),
        Some(GuestAction::Exit(128 + 9))
    );
}

/// A fatal signal that is not SIGKILL waits until the stopped group goes on, and kills it then
/// (`wants_signal` passes stopped tasks by), as a handled one runs its handler then.
#[test]
fn other_signals_wait_for_sigcont() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    handle(&kernel, &caller, Signal::SIGUSR1);
    kernel.kill(task, Signal::SIGSTOP).unwrap();
    assert_eq!(kernel.take_pending(task), Some(GuestAction::Stopped));

    kernel.kill(task, Signal::SIGUSR1).unwrap();
    kernel.kill(task, Signal::SIGTERM).unwrap();
    assert!(waits_stopped(&kernel, task));
    assert!(!kernel.process(task).group.exiting());

    kernel.kill(task, Signal::SIGCONT).unwrap();
    assert!(!waits_stopped(&kernel, task));
    assert!(matches!(
        kernel.take_pending(task),
        Some(GuestAction::Signal(frame)) if frame.signo() == Signal::SIGUSR1.0
    ));
    assert_eq!(kernel.take_pending(task), Some(GuestAction::Exit(128 + 15)));
}

/// A task with a fatal signal pending dies of it instead of taking its part in a stop, as
/// Linux's `complete_signal` makes the group exit as the signal is sent; the stopped thread
/// wakes to its exit.
#[test]
fn a_fatal_signal_pending_beats_a_stop_under_way() {
    let (kernel, leader, memory) = machine();
    let caller = Caller::new(leader, &memory);
    let thread = spawn_thread(&kernel, &caller);
    kernel.kill(leader, Signal::SIGSTOP).unwrap();
    assert_eq!(kernel.take_pending(leader), Some(GuestAction::Stopped));

    kernel.kill(leader, Signal::SIGTERM).unwrap();

    assert_eq!(
        kernel.take_pending(thread),
        Some(GuestAction::Exit(128 + 15))
    );
    assert!(!waits_stopped(&kernel, leader));
    assert_eq!(
        kernel.take_pending(leader),
        Some(GuestAction::Exit(128 + 15))
    );
}

/// A handler for a stop signal runs instead of the stop.
#[test]
fn a_handled_stop_signal_does_not_stop() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    handle(&kernel, &caller, Signal::SIGTSTP);

    kernel.kill(task, Signal::SIGTSTP).unwrap();

    assert!(matches!(
        kernel.take_pending(task),
        Some(GuestAction::Signal(frame)) if frame.signo() == Signal::SIGTSTP.0
    ));
    assert!(!group_stopped(&kernel, task));
}

/// `task_join_group_stop`: a thread created while its group stops takes part in the stop, and
/// `exit_signals`: one that exits takes its part on its way out.
#[test]
fn threads_that_start_or_exit_during_a_stop_take_their_part() {
    let (kernel, leader, memory) = machine();
    let caller = Caller::new(leader, &memory);
    let exiting = spawn_thread(&kernel, &caller);
    let stopping = spawn_thread(&kernel, &caller);
    // Sent to the thread, which starts the group stop while the leader runs on.
    let pid = leader.0 as Pid;
    let stop = kernel.tgkill(&caller, pid, stopping.0 as Pid, Signal::SIGSTOP);
    assert_eq!(block_on(stop), Ok(0));
    assert_eq!(kernel.take_pending(stopping), Some(GuestAction::Stopped));

    // The leader, still running, starts a thread before it notices.
    let spawn = kernel.thread_spawn(&caller, THREAD, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL);
    let started = TaskId(block_on(spawn).unwrap() as u32);
    assert!(matches!(
        kernel.take_pending(leader),
        Some(GuestAction::StartThread { .. })
    ));
    assert_eq!(kernel.take_pending(leader), Some(GuestAction::Stopped));
    let exiting_caller = Caller::new(exiting, &memory);
    assert_eq!(block_on(kernel.exit(&exiting_caller, 0)), Ok(()));
    assert_eq!(kernel.take_pending(exiting), Some(GuestAction::Exit(0)));
    assert!(
        !group_stopped(&kernel, leader),
        "the new thread has yet to stop"
    );

    assert_eq!(kernel.take_pending(started), Some(GuestAction::Stopped));
    assert!(group_stopped(&kernel, leader));
}

/// An `execve` from a thread ends the stop under way (`zap_other_threads`): the stopped leader
/// goes on to take the new image, and nothing stops while the `execve` runs.
#[test]
fn an_execve_ends_a_stop_under_way() {
    let (kernel, leader, memory) = machine();
    let caller = Caller::new(leader, &memory);
    install_prog(&kernel, &caller, &memory);
    let execing = spawn_thread(&kernel, &caller);
    kernel.kill(leader, Signal::SIGSTOP).unwrap();
    assert_eq!(kernel.take_pending(leader), Some(GuestAction::Stopped));
    assert!(waits_stopped(&kernel, leader));

    let thread = Caller::new(execing, &memory);
    let mut exec = Box::pin(execve(&kernel, &thread));
    assert_eq!(exec.as_mut().now_or_never(), None);

    assert!(!waits_stopped(&kernel, leader));
    kernel.kill(leader, Signal::SIGSTOP).unwrap();
    let handed = GuestAction::Exec { vfork_parent: None };
    assert_eq!(kernel.take_pending(leader), Some(handed));
    let mut wait = Box::pin(kernel.exec_wait(leader));
    assert_eq!(wait.as_mut().now_or_never(), None);
    assert_eq!(exec.as_mut().now_or_never(), Some(Ok(0)));
    assert_eq!(kernel.take_pending(execing), Some(GuestAction::Exit(0)));
    kernel.release(execing);
    assert_eq!(wait.as_mut().now_or_never(), Some(Ok(())));
    assert!(!group_stopped(&kernel, leader));
}

// ---- reports to the parent ----

const SIGCHLD_BIT: u64 = 1 << (Signal::SIGCHLD.0 - 1);

/// An address space the engine can duplicate, as the asyncify configuration's is.
fn forkable() -> ForkableMemory {
    ForkableMemory(FlatMemory(RefCell::new(vec![0; 0x1000])))
}

/// Forks the caller and returns the child.
fn fork(kernel: &Kernel, caller: &Caller<'_>) -> TaskId {
    let flags = Signal::SIGCHLD.0 as u32;
    let clone = kernel.clone(caller, flags, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL);
    let child = TaskId(block_on(clone).expect("fork") as u32);
    while kernel.take_pending(caller.task()).is_some() {}
    child
}

/// A parent that keeps the SIGCHLDs it gets pending, and a child it forked, on a kernel with
/// init, so that the parent is not pid 1, whose children no process group counts as linked
/// outside (`is_global_init`).
fn family() -> (Kernel, TaskId, ForkableMemory, TaskId) {
    let stdin = Arc::new(Input(Mutex::new(Vec::new())));
    let (kernel, parent, _) = spawn_on(test_kernel().with_init(), stdin, &[], &[]);
    let memory = forkable();
    let child = fork(&kernel, &Caller::new(parent, &memory));
    kernel.task_signals(parent).replace_blocked(SIGCHLD_BIT);
    (kernel, parent, memory, child)
}

/// `waitid(P_PID, child, options | WNOHANG)`: the reported `(si_code, si_status)`, if any.
fn wait(kernel: &Kernel, caller: &Caller<'_>, child: TaskId, options: u32) -> Option<(i32, i32)> {
    let options = WaitOptions(options | WaitOptions::WNOHANG.0);
    let infop = SiginfoPtr::new(0x300);
    let wait = kernel.waitid(
        caller,
        Idtype::P_PID,
        child.0 as Pid,
        infop,
        options,
        RusagePtr::new(0),
    );
    assert_eq!(block_on(wait), Ok(0));
    let info: Siginfo = block_on(read_value(caller.memory(), GuestPtr::new(0x300))).unwrap();
    (info.si_signo != 0).then(|| (info.si_code, field(&info, 8)))
}

fn field(info: &Siginfo, at: usize) -> i32 {
    i32::from_le_bytes(info.sifields[at..at + 4].try_into().unwrap())
}

/// The SIGCHLD pending for `parent`: `(si_code, si_pid, si_status)`.
fn sigchld(kernel: &Kernel, parent: TaskId) -> Option<(i32, i32, i32)> {
    let info = kernel.task_signals(parent).dequeue_from(SIGCHLD_BIT)?;
    Some((info.si_code, field(&info, 0), field(&info, 8)))
}

const STOPPED: i32 = SiCode::CLD_STOPPED.0;
const CONTINUED: i32 = SiCode::CLD_CONTINUED.0;
const WSTOPPED: u32 = WaitOptions::WSTOPPED.0;
const WCONTINUED: u32 = WaitOptions::WCONTINUED.0;
const WNOWAIT: u32 = WaitOptions::WNOWAIT.0;

/// `do_notify_parent_cldstop` and `wait_task_stopped`, `wait_task_continued`: the parent gets
/// SIGCHLD with `CLD_STOPPED` and then `CLD_CONTINUED`, and `waitid` reports each once, or
/// again after `WNOWAIT`.
#[test]
fn the_parent_hears_of_a_stop_and_a_continue_once() {
    let (kernel, parent, memory, child) = family();
    let caller = Caller::new(parent, &memory);
    let pid = child.0 as i32;

    kernel.kill(child, Signal::SIGSTOP).unwrap();
    assert_eq!(kernel.take_pending(child), Some(GuestAction::Stopped));

    assert_eq!(
        sigchld(&kernel, parent),
        Some((STOPPED, pid, Signal::SIGSTOP.0))
    );
    assert_eq!(wait(&kernel, &caller, child, WaitOptions::WEXITED.0), None);
    let stopped = Some((STOPPED, Signal::SIGSTOP.0));
    assert_eq!(wait(&kernel, &caller, child, WSTOPPED | WNOWAIT), stopped);
    assert_eq!(wait(&kernel, &caller, child, WSTOPPED), stopped);
    assert_eq!(
        wait(&kernel, &caller, child, WSTOPPED),
        None,
        "reported once"
    );
    assert_eq!(wait(&kernel, &caller, child, WCONTINUED), None);

    kernel.kill(child, Signal::SIGCONT).unwrap();
    assert_eq!(kernel.take_pending(child), None);

    let continued = Some((CONTINUED, Signal::SIGCONT.0));
    assert_eq!(
        sigchld(&kernel, parent),
        Some((CONTINUED, pid, Signal::SIGCONT.0))
    );
    assert_eq!(wait(&kernel, &caller, child, WSTOPPED), None);
    assert_eq!(
        wait(&kernel, &caller, child, WCONTINUED | WNOWAIT),
        continued
    );
    assert_eq!(wait(&kernel, &caller, child, WCONTINUED), continued);
    assert_eq!(
        wait(&kernel, &caller, child, WCONTINUED),
        None,
        "reported once"
    );
}

/// `SA_NOCLDSTOP` keeps SIGCHLD from coming for stops and continues; `waitid` still reports
/// them, and a parent blocked in it wakes.
#[test]
fn sa_nocldstop_keeps_sigchld_away_but_not_waitid() {
    let (kernel, parent, memory, child) = family();
    let caller = Caller::new(parent, &memory);
    let act = Sigaction {
        sa_handler: 7,
        sa_flags: SaFlags::SA_NOCLDSTOP.0,
        sa_mask: Sigset { sig: [0; 2] },
    };
    block_on(write_value(caller.memory(), GuestPtrMut::new(0x140), &act)).unwrap();
    assert_eq!(block_on(kernel.signal_callback_register(&caller, 3)), Ok(0));
    let set = kernel.rt_sigaction(
        &caller,
        Signal::SIGCHLD,
        GuestPtr::new(0x140),
        GuestPtrMut::NULL,
        8,
    );
    assert_eq!(block_on(set), Ok(0));
    let options = WaitOptions(WSTOPPED);
    let infop = SiginfoPtr::new(0x300);
    let mut waiting =
        Box::pin(kernel.waitid(&caller, Idtype::P_ALL, 0, infop, options, RusagePtr::new(0)));
    assert_eq!(waiting.as_mut().now_or_never(), None);

    kernel.kill(child, Signal::SIGSTOP).unwrap();
    assert_eq!(kernel.take_pending(child), Some(GuestAction::Stopped));

    assert_eq!(waiting.as_mut().now_or_never(), Some(Ok(0)));
    let info: Siginfo = block_on(read_value(caller.memory(), GuestPtr::new(0x300))).unwrap();
    assert_eq!(
        (info.si_code, field(&info, 8)),
        (STOPPED, Signal::SIGSTOP.0)
    );
    kernel.kill(child, Signal::SIGCONT).unwrap();
    assert_eq!(kernel.take_pending(child), None);
    assert_eq!(sigchld(&kernel, parent), None);
    let continued = Some((CONTINUED, Signal::SIGCONT.0));
    assert_eq!(wait(&kernel, &caller, child, WCONTINUED), continued);
}

/// SIGCONT before every thread stopped: the parent hears of a stop, with no signal, since
/// SIGCHLD does not queue and the continue would be lost behind it (`prepare_signal`); the
/// continue is still there for `WCONTINUED`, and the stop never completes for `WSTOPPED`.
#[test]
fn a_stop_cut_short_is_reported_as_a_stop() {
    let (kernel, parent, memory, child) = family();
    let caller = Caller::new(parent, &memory);
    let child_memory = forkable();
    let child_caller = Caller::new(child, &child_memory);
    let thread = spawn_thread(&kernel, &child_caller);
    let pid = child.0 as Pid;
    let stop = kernel.tgkill(&child_caller, pid, thread.0 as Pid, Signal::SIGSTOP);
    assert_eq!(block_on(stop), Ok(0));
    assert_eq!(kernel.take_pending(thread), Some(GuestAction::Stopped));

    kernel.kill(child, Signal::SIGCONT).unwrap();
    assert_eq!(kernel.take_pending(child), None);

    assert_eq!(sigchld(&kernel, parent), Some((STOPPED, child.0 as i32, 0)));
    assert_eq!(wait(&kernel, &caller, child, WSTOPPED), None);
    let continued = Some((CONTINUED, Signal::SIGCONT.0));
    assert_eq!(wait(&kernel, &caller, child, WCONTINUED), continued);
    assert_eq!(kernel.take_pending(thread), None);
    assert_eq!(sigchld(&kernel, parent), None, "told once");
}

// ---- orphaned process groups ----

/// A process group with no parent outside it in its session is orphaned: SIGTSTP, SIGTTIN and
/// SIGTTOU do nothing there, SIGSTOP stops as anywhere (`get_signal`).
#[test]
fn terminal_stop_signals_do_nothing_in_an_orphaned_process_group() {
    let (kernel, _, _, child) = family();
    let child_memory = forkable();
    let child_caller = Caller::new(child, &child_memory);

    // In its own group, in its parent's session: not orphaned.
    assert_eq!(block_on(kernel.setpgid(&child_caller, 0, 0)), Ok(0));
    kernel.kill(child, Signal::SIGTSTP).unwrap();
    assert_eq!(kernel.take_pending(child), Some(GuestAction::Stopped));
    kernel.kill(child, Signal::SIGCONT).unwrap();
    assert_eq!(kernel.take_pending(child), None);

    // A session of its own: orphaned.
    assert_eq!(
        block_on(kernel.setsid(&child_caller)),
        Err(SysErr::Errno(Errno::EPERM))
    );
    let grandchild = fork(&kernel, &child_caller);
    let grandchild_memory = forkable();
    let grandchild_caller = Caller::new(grandchild, &grandchild_memory);
    assert_eq!(
        block_on(kernel.setsid(&grandchild_caller)),
        Ok(grandchild.0 as Pid)
    );
    for sig in [Signal::SIGTSTP, Signal::SIGTTIN, Signal::SIGTTOU] {
        kernel.kill(grandchild, sig).unwrap();
        assert_eq!(kernel.take_pending(grandchild), None, "{sig:?}");
    }
    kernel.kill(grandchild, Signal::SIGSTOP).unwrap();
    assert_eq!(kernel.take_pending(grandchild), Some(GuestAction::Stopped));
}

/// `kill_orphaned_pgrp`: when a parent's exit orphans its child's process group, which has a
/// stopped process, the group gets SIGHUP then SIGCONT, so the stopped one goes on and dies.
#[test]
fn a_stopped_job_its_parent_orphans_gets_sighup_and_sigcont() {
    let (kernel, parent, memory, child) = family();
    let caller = Caller::new(parent, &memory);
    let child_memory = forkable();
    let child_caller = Caller::new(child, &child_memory);
    assert_eq!(block_on(kernel.setpgid(&child_caller, 0, 0)), Ok(0));
    let sibling = fork(&kernel, &child_caller);
    kernel.kill(child, Signal::SIGSTOP).unwrap();
    assert_eq!(kernel.take_pending(child), Some(GuestAction::Stopped));

    assert_eq!(block_on(kernel.exit_group(&caller, 0)), Ok(()));

    assert!(!waits_stopped(&kernel, child));
    assert_eq!(kernel.take_pending(child), Some(GuestAction::Exit(128 + 1)));
    assert_eq!(
        kernel.take_pending(sibling),
        Some(GuestAction::Exit(128 + 1))
    );
}

/// A group that is orphaned but has no stopped process gets nothing.
#[test]
fn an_orphaned_group_with_nothing_stopped_gets_no_sighup() {
    let (kernel, parent, memory, child) = family();
    let caller = Caller::new(parent, &memory);
    let child_memory = forkable();
    assert_eq!(
        block_on(kernel.setpgid(&Caller::new(child, &child_memory), 0, 0)),
        Ok(0)
    );

    assert_eq!(block_on(kernel.exit_group(&caller, 0)), Ok(()));

    assert_eq!(kernel.take_pending(child), None);
}
