//! Task lifetime: the work flag, releasing tasks, `de_thread`, faults and `restart_syscall`.

use futures::FutureExt as _;
use wasixv2_abi::{FutexOp, KernelTimespec, Signal};
use wasixv2_syscalls::{read_value, write_value};

use super::{ipc::machine, *};

/// The flags `pthread_create` passes.
const THREAD: u32 = 0x10f00;

fn spawn_thread(kernel: &Kernel, caller: &Caller<'_>) -> TaskId {
    let tid =
        block_on(kernel.thread_spawn(caller, THREAD, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL))
            .unwrap();
    TaskId(tid as u32)
}

fn live(kernel: &Kernel, task: TaskId) -> bool {
    kernel.state.lock().unwrap().tasks.contains_key(&task)
}

#[test]
fn the_work_flag_says_when_there_is_work() {
    let (kernel, task, _) = setup(b"");
    let flag = kernel.work_flag(task);
    assert!(!flag.is_raised());

    kernel.kill(task, Signal::SIGTERM).unwrap();

    assert!(flag.is_raised());
    assert_eq!(kernel.take_pending(task), Some(GuestAction::Exit(128 + 15)));
    assert_eq!(kernel.take_pending(task), None);
    assert!(!flag.is_raised());
}

#[test]
fn a_thread_spawn_survives_a_later_action_and_an_exit_aborts_it() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let child = spawn_thread(&kernel, &caller);

    block_on(kernel.exit_group(&caller, 4)).unwrap();

    // The start was queued first, but the thread will never run: the exit takes its place.
    assert_eq!(kernel.take_pending(task), Some(GuestAction::Exit(4)));
    assert!(!live(&kernel, child));
}

#[test]
fn released_tasks_are_forgotten() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let child = spawn_thread(&kernel, &caller);
    assert!(matches!(
        kernel.take_pending(task),
        Some(GuestAction::StartThread { .. })
    ));

    block_on(kernel.exit(&Caller::new(child, &memory), 0)).unwrap();
    assert_eq!(kernel.take_pending(child), Some(GuestAction::Exit(0)));
    kernel.release(child);
    block_on(kernel.exit_group(&caller, 0)).unwrap();
    kernel.release(task);

    assert!(kernel.state.lock().unwrap().tasks.is_empty());
}

#[test]
fn a_fault_kills_the_group_with_its_signal() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let child = spawn_thread(&kernel, &caller);
    let _ = kernel.take_pending(task);

    kernel.fault(child, Signal::SIGSEGV.0);

    assert_eq!(kernel.take_pending(task), Some(GuestAction::Exit(128 + 11)));
    assert_eq!(
        kernel.take_pending(child),
        Some(GuestAction::Exit(128 + 11))
    );
}

#[test]
fn execve_from_a_thread_kills_the_others_and_runs_as_the_leader() {
    let (kernel, leader, memory) = machine();
    let caller = Caller::new(leader, &memory);
    let execing = spawn_thread(&kernel, &caller);
    let other = spawn_thread(&kernel, &caller);
    while kernel.take_pending(leader).is_some() {}
    // The exec'ing thread blocks SIGUSR1; the leader goes on with that mask.
    kernel.task_signals(execing).replace_blocked(1 << 9);

    assert_eq!(kernel.de_thread(execing), leader);

    assert_eq!(kernel.take_pending(execing), Some(GuestAction::Exit(0)));
    assert_eq!(kernel.take_pending(other), Some(GuestAction::Exit(0)));
    assert_eq!(kernel.task_signals(leader).blocked(), 1 << 9);
    // The image waits for the others to unwind.
    let mut wait = Box::pin(kernel.exec_wait(leader));
    assert_eq!(wait.as_mut().now_or_never(), None);
    kernel.release(execing);
    assert_eq!(wait.as_mut().now_or_never(), None);
    kernel.release(other);
    assert_eq!(wait.as_mut().now_or_never(), Some(Ok(())));
    // The leader is the only task left, and its exit ends the group.
    block_on(kernel.exit(&caller, 5)).unwrap();
    assert_eq!(kernel.take_pending(leader), Some(GuestAction::Exit(5)));
}

#[test]
fn an_interrupted_nanosleep_writes_the_time_left_and_restarts_with_it() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let request = KernelTimespec {
        tv_sec: 5,
        tv_nsec: 0,
    };
    block_on(write_value(&memory, GuestPtrMut::new(0x100), &request)).unwrap();
    let sleep = || {
        block_on(kernel.clock_nanosleep_time64(
            &caller,
            ClockId::CLOCK_MONOTONIC,
            TimerFlags(0),
            GuestPtr::new(0x100),
            GuestPtrMut::new(0x200),
        ))
    };
    // A signal is pending, so the sleep ends at once.
    kernel.kill(task, Signal::SIGUSR1).unwrap();

    assert_eq!(sleep(), Err(SysErr::RestartBlock));
    let left: KernelTimespec = block_on(read_value(&memory, GuestPtr::new(0x200))).unwrap();
    assert_eq!((left.tv_sec, left.tv_nsec), (5, 0));
    // Without a handler, the call goes on through `restart_syscall`; with the signal blocked
    // now, it sleeps to the end.
    kernel.task_signals(task).replace_blocked(1 << 9);
    assert_eq!(block_on(kernel.restart_syscall(&caller)), Ok(0));
    // Nothing is left to restart.
    assert_eq!(
        block_on(kernel.restart_syscall(&caller)),
        Err(SysErr::Errno(Errno::EINTR))
    );
}

#[test]
fn an_interrupted_timed_futex_wait_restarts_to_its_deadline() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let timeout = KernelTimespec {
        tv_sec: 1,
        tv_nsec: 0,
    };
    block_on(write_value(&memory, GuestPtrMut::new(0x100), &timeout)).unwrap();
    kernel.kill(task, Signal::SIGUSR1).unwrap();

    let wait = block_on(kernel.futex_time64(
        &caller,
        GuestPtrMut::new(0x200),
        FutexOp::FUTEX_WAIT,
        0,
        0x100,
        GuestPtrMut::NULL,
        0,
    ));

    assert_eq!(wait, Err(SysErr::RestartBlock));
    kernel.task_signals(task).replace_blocked(1 << 9);
    // The test host's clock stands still and its timers fire at once.
    assert_eq!(
        block_on(kernel.restart_syscall(&caller)),
        Err(SysErr::Errno(Errno::ETIMEDOUT))
    );
}
