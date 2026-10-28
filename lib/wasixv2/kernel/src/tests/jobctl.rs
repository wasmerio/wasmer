//! Job control: group stops and SIGCONT.

use futures::FutureExt as _;
use wasixv2_abi::{OpenFlags, Signal};
use wasixv2_wait::Interrupt as _;

use super::{
    ipc::{machine, pipe},
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
    kernel.kill(leader, Signal::SIGSTOP).unwrap();
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
