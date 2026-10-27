//! Task lifetime: the work flag, releasing tasks, `de_thread`, faults and `restart_syscall`.

use futures::FutureExt as _;
use wasixv2_abi::{FutexOp, KernelTimespec, Pid, Sigaction, Signal, Sigset};
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

/// `de_thread`: an `execve` from a thread kills the others, and commits only once no other
/// thread runs the old image; the leader then runs the new one with the thread's signal state.
#[test]
fn execve_from_a_thread_commits_once_the_old_image_is_gone_and_runs_as_the_leader() {
    let (kernel, leader, memory) = machine();
    let caller = Caller::new(leader, &memory);
    install_prog(&kernel, &caller, &memory);
    let execing = spawn_thread(&kernel, &caller);
    let other = spawn_thread(&kernel, &caller);
    while kernel.take_pending(leader).is_some() {}
    // The exec'ing thread blocks SIGUSR1; the leader goes on with that mask.
    kernel.task_signals(execing).replace_blocked(1 << 9);
    let thread = Caller::new(execing, &memory);
    let mut exec = Box::pin(execve(&kernel, &thread));

    // The others are killed, and the leader leaves its old image for the new one.
    assert_eq!(exec.as_mut().now_or_never(), None);
    assert_eq!(kernel.take_pending(other), Some(GuestAction::Exit(0)));
    let handed = GuestAction::Exec { vfork_parent: None };
    assert_eq!(kernel.take_pending(leader), Some(handed));
    assert_eq!(kernel.take_image(leader).as_deref(), Some(IMAGE));
    // Nothing is committed while another thread may still run the old image.
    kernel.release(other);
    assert_eq!(exec.as_mut().now_or_never(), None);
    assert!(kernel.process(leader).exe.lock().unwrap().is_none());
    let mut wait = Box::pin(kernel.exec_wait(leader));
    assert_eq!(wait.as_mut().now_or_never(), None);
    assert_eq!(exec.as_mut().now_or_never(), Some(Ok(0)));
    assert!(kernel.process(leader).exe.lock().unwrap().is_some());
    assert_eq!(kernel.task_signals(leader).blocked(), 1 << 9);
    // The exec'ing thread is done, and the image starts once it is gone.
    assert_eq!(kernel.take_pending(execing), Some(GuestAction::Exit(0)));
    assert_eq!(wait.as_mut().now_or_never(), None);
    kernel.release(execing);
    assert_eq!(wait.as_mut().now_or_never(), Some(Ok(())));
    // The leader is the only task left, and its exit ends the group.
    block_on(kernel.exit(&caller, 5)).unwrap();
    assert_eq!(kernel.take_pending(leader), Some(GuestAction::Exit(5)));
}

/// The wait of an `execve` for the other threads is killable: a handled signal does not end
/// it, a fatal one ends the group with that signal and the `execve` with EAGAIN.
#[test]
fn a_fatal_signal_ends_an_execve_waiting_for_the_other_threads() {
    let (kernel, leader, memory) = machine();
    let caller = Caller::new(leader, &memory);
    install_prog(&kernel, &caller, &memory);
    handle(&kernel, &caller, Signal::SIGUSR1);
    let other = spawn_thread(&kernel, &caller);
    while kernel.take_pending(leader).is_some() {}
    let mut exec = Box::pin(execve(&kernel, &caller));
    assert_eq!(exec.as_mut().now_or_never(), None);

    kernel.kill(leader, Signal::SIGUSR1).unwrap();
    assert_eq!(exec.as_mut().now_or_never(), None);
    kernel.kill(leader, Signal::SIGKILL).unwrap();

    assert_eq!(
        exec.as_mut().now_or_never(),
        Some(Err(SysErr::Errno(Errno::EAGAIN)))
    );
    assert!(kernel.process(leader).exe.lock().unwrap().is_none());
    assert_eq!(
        kernel.take_pending(leader),
        Some(GuestAction::Exit(128 + 9))
    );
    assert_eq!(kernel.take_pending(other), Some(GuestAction::Exit(0)));
}

/// While an `execve` kills the other threads, the leader still runs its old image, and the
/// threads it would start now would outlive the exec (Linux's leader has SIGKILL pending).
#[test]
fn the_leader_starts_no_task_while_another_threads_execve_ends_the_old_image() {
    let (kernel, leader, memory) = machine();
    let caller = Caller::new(leader, &memory);
    install_prog(&kernel, &caller, &memory);
    let execing = spawn_thread(&kernel, &caller);
    while kernel.take_pending(leader).is_some() {}
    let thread = Caller::new(execing, &memory);
    let mut exec = Box::pin(execve(&kernel, &thread));
    assert_eq!(exec.as_mut().now_or_never(), None);

    let spawn = kernel.thread_spawn(&caller, THREAD, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL);
    assert_eq!(block_on(spawn), Err(SysErr::Errno(Errno::EINTR)));
    let vfork = 0x4000 | 0x100 | Signal::SIGCHLD.0 as u32;
    let vfork = kernel.clone(&caller, vfork, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL);
    assert_eq!(block_on(vfork), Err(SysErr::Errno(Errno::EINTR)));
}

/// A leader that called `exit` waits as a zombie for its group; another thread's `execve`
/// gives it the new image instead.
#[test]
fn an_execve_from_a_thread_revives_a_zombie_leader() {
    let (kernel, leader, memory) = machine();
    let caller = Caller::new(leader, &memory);
    install_prog(&kernel, &caller, &memory);
    let execing = spawn_thread(&kernel, &caller);
    while kernel.take_pending(leader).is_some() {}
    let mut exit = Box::pin(kernel.exit(&caller, 7));
    assert_eq!(exit.as_mut().now_or_never(), None);
    let thread = Caller::new(execing, &memory);
    let mut exec = Box::pin(execve(&kernel, &thread));
    assert_eq!(exec.as_mut().now_or_never(), None);

    assert_eq!(exit.as_mut().now_or_never(), Some(Ok(())));
    let handed = GuestAction::Exec { vfork_parent: None };
    assert_eq!(kernel.take_pending(leader), Some(handed));
    let mut wait = Box::pin(kernel.exec_wait(leader));
    assert_eq!(wait.as_mut().now_or_never(), None);
    assert_eq!(exec.as_mut().now_or_never(), Some(Ok(0)));
    assert_eq!(kernel.take_pending(execing), Some(GuestAction::Exit(0)));
    kernel.release(execing);

    assert_eq!(wait.as_mut().now_or_never(), Some(Ok(())));
    assert!(!kernel.process(leader).group.exiting());
}

/// A leader in `sigsuspend` while another thread's `execve` ends the old image: the end of its
/// temporary mask does not put its own mask back over the one the new image inherits from the
/// exec'ing thread.
#[test]
fn the_leaders_temporary_mask_does_not_outlive_another_threads_execve() {
    const SIGUSR1: u64 = 1 << 9;
    const SIGTERM: u64 = 1 << 14;
    let (kernel, leader, memory) = machine();
    let caller = Caller::new(leader, &memory);
    install_prog(&kernel, &caller, &memory);
    let execing = spawn_thread(&kernel, &caller);
    while kernel.take_pending(leader).is_some() {}
    kernel.task_signals(leader).replace_blocked(SIGTERM);
    kernel.task_signals(execing).replace_blocked(SIGUSR1);
    put(&memory, 0x160, &[0; 8]);
    let mut suspend = Box::pin(kernel.rt_sigsuspend(&caller, GuestPtr::new(0x160), 8));
    assert_eq!(suspend.as_mut().now_or_never(), None);
    let thread = Caller::new(execing, &memory);
    let mut exec = Box::pin(execve(&kernel, &thread));

    assert_eq!(exec.as_mut().now_or_never(), None);
    assert!(suspend.as_mut().now_or_never().is_some());
    let handed = GuestAction::Exec { vfork_parent: None };
    assert_eq!(kernel.take_pending(leader), Some(handed));
    let mut wait = Box::pin(kernel.exec_wait(leader));
    assert_eq!(wait.as_mut().now_or_never(), None);
    assert_eq!(exec.as_mut().now_or_never(), Some(Ok(0)));
    assert_eq!(kernel.take_pending(execing), Some(GuestAction::Exit(0)));
    kernel.release(execing);
    assert_eq!(wait.as_mut().now_or_never(), Some(Ok(())));

    // The new image's first return to the guest has no temporary mask to end.
    assert_eq!(kernel.take_pending(leader), None);
    assert_eq!(kernel.task_signals(leader).blocked(), SIGUSR1);
}

const IMAGE: &[u8] = b"\0asm\x01\0\0\0";

/// Writes an executable `/prog` holding [`IMAGE`], and `execve`'s arguments for it: the path
/// at 0x2000, argv (`prog`) at 0x2010, an empty envp at 0x2018.
fn install_prog(kernel: &Kernel, caller: &Caller<'_>, memory: &FlatMemory) {
    put(memory, 0x2000, b"/prog\0");
    put(memory, 0x2010, &0x2000u32.to_le_bytes());
    put(memory, 0x2100, IMAGE);
    let create = OpenFlags::O_CREAT | OpenFlags::O_WRONLY;
    let open = kernel.openat(caller, Fd(-100), GuestPtr::new(0x2000), create, 0o755);
    let fd = block_on(open).unwrap();
    let write = kernel.write(caller, fd, UserSlice::new(0x2100, IMAGE.len() as u32));
    assert_eq!(block_on(write), Ok(IMAGE.len() as u32));
    assert_eq!(block_on(kernel.close(caller, fd)), Ok(0));
}

/// `execve("/prog", ["prog"], [])` from `caller`, after [`install_prog`].
fn execve<'a>(
    kernel: &'a Kernel,
    caller: &'a Caller<'a>,
) -> impl std::future::Future<Output = SysResult<i32>> + 'a {
    kernel.execve(
        caller,
        GuestPtr::new(0x2000),
        GuestPtr::new(0x2010),
        GuestPtr::new(0x2018),
    )
}

/// Installs handler 7 for `sig`, behind trampoline 3.
fn handle(kernel: &Kernel, caller: &Caller<'_>, sig: Signal) {
    let act = Sigaction {
        sa_handler: 7,
        sa_flags: 0,
        sa_mask: Sigset { sig: [0; 2] },
    };
    block_on(write_value(caller.memory(), GuestPtrMut::new(0x140), &act)).unwrap();
    assert_eq!(block_on(kernel.signal_callback_register(caller, 3)), Ok(0));
    let set = kernel.rt_sigaction(caller, sig, GuestPtr::new(0x140), GuestPtrMut::NULL, 8);
    assert_eq!(block_on(set), Ok(0));
}

/// `execve` is async-signal-safe: a thread other than the leader calls it from a handler. The
/// leader takes the thread's signal state over (`de_thread`), and the handler unwinds with the
/// old image after that, which once panicked the host.
#[test]
fn an_execve_from_a_threads_handler_unwinds_after_the_leader_took_over() {
    let (kernel, leader, memory) = machine();
    let caller = Caller::new(leader, &memory);
    install_prog(&kernel, &caller, &memory);
    let execing = spawn_thread(&kernel, &caller);
    while kernel.take_pending(leader).is_some() {}
    handle(&kernel, &caller, Signal::SIGUSR1);
    let pid = leader.0 as Pid;
    let kill = kernel.tgkill(&caller, pid, execing.0 as Pid, Signal::SIGUSR1);
    assert_eq!(block_on(kill), Ok(0));
    assert!(matches!(
        kernel.take_pending(execing),
        Some(GuestAction::Signal(_))
    ));
    let thread = Caller::new(execing, &memory);
    let mut exec = Box::pin(execve(&kernel, &thread));
    assert_eq!(exec.as_mut().now_or_never(), None);
    let _ = kernel.take_pending(leader);
    let mut wait = Box::pin(kernel.exec_wait(leader));
    assert_eq!(wait.as_mut().now_or_never(), None);

    assert_eq!(exec.as_mut().now_or_never(), Some(Ok(0)));
    Control::signal_return(&kernel, execing, None);

    assert_eq!(kernel.take_pending(execing), Some(GuestAction::Exit(0)));
    kernel.release(execing);
    assert_eq!(wait.as_mut().now_or_never(), Some(Ok(())));
}

/// `execve` in a group that is exiting fails before the point of no return (`de_thread`'s
/// EAGAIN): nothing of the new image is committed, and the exit ends the caller.
#[test]
fn an_execve_in_an_exiting_group_fails_with_eagain() {
    let (kernel, leader, memory) = machine();
    let caller = Caller::new(leader, &memory);
    install_prog(&kernel, &caller, &memory);
    let thread = spawn_thread(&kernel, &caller);
    while kernel.take_pending(leader).is_some() {}
    kernel.do_group_exit(leader, 3);

    let ret = block_on(execve(&kernel, &Caller::new(thread, &memory)));

    assert_eq!(ret, Err(SysErr::Errno(Errno::EAGAIN)));
    assert!(kernel.process(leader).exe.lock().unwrap().is_none());
    assert_eq!(kernel.take_pending(thread), Some(GuestAction::Exit(3)));
    assert_eq!(kernel.take_pending(thread), None);
    assert_eq!(kernel.take_pending(leader), Some(GuestAction::Exit(3)));
    assert_eq!(kernel.take_pending(leader), None);
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
