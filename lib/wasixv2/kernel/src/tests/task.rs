//! Task lifetime: the work flag, releasing tasks, `de_thread`, faults and `restart_syscall`.

use futures::FutureExt as _;
use wasixv2_abi::{CloneFlags, FutexOp, KernelTimespec, Pid, Sigaction, Signal, Sigset};
use wasixv2_syscalls::{Image, Resume, VforkParent, read_value, write_value};

use super::{
    ipc::{errno, machine},
    *,
};
use crate::sync::MutexExt;

/// The flags `pthread_create` passes.
const THREAD: u32 = 0x10f00;

fn spawn_thread(kernel: &Kernel, caller: &Caller<'_>) -> TaskId {
    let tid =
        block_on(kernel.thread_spawn(caller, THREAD, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL))
            .unwrap();
    TaskId(tid as u32)
}

fn live(kernel: &Kernel, task: TaskId) -> bool {
    kernel.state.lock_or_recover().tasks.contains_key(&task)
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

    assert!(kernel.state.lock_or_recover().tasks.is_empty());
}

/// A vfork whose caller's exit was queued first, as another thread's `exit_group` can between
/// `clone`'s check and its queueing, never starts its child, which goes with its process.
#[test]
fn a_vfork_behind_an_exit_forgets_its_child() {
    const VFORK: u32 = 0x4000 | 0x100 | Signal::SIGCHLD.0 as u32;
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    kernel.queue(task, GuestAction::Exit(9));

    let vfork = kernel.clone(&caller, VFORK, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL);
    assert_eq!(block_on(vfork), Ok(0));

    let child = TaskId(task.0 + 1);
    assert_eq!(kernel.take_pending(task), Some(GuestAction::Exit(9)));
    assert_eq!(kernel.take_pending(task), None);
    assert!(!live(&kernel, child));
    assert!(kernel.procs.find(child.0).is_none());
    assert!(kernel.procs.tree.lock_or_recover().vforks.is_empty());
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
    let image = kernel.take_image(leader);
    assert_eq!(image.as_ref().map(Image::bytes), Some(IMAGE));
    // Nothing is committed while another thread may still run the old image.
    kernel.release(other);
    assert_eq!(exec.as_mut().now_or_never(), None);
    assert!(kernel.process(leader).exe.lock_or_recover().is_none());
    let mut wait = Box::pin(kernel.exec_wait(leader));
    assert_eq!(wait.as_mut().now_or_never(), None);
    assert_eq!(exec.as_mut().now_or_never(), Some(Ok(0)));
    assert!(kernel.process(leader).exe.lock_or_recover().is_some());
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

/// The threads an `execve` ends release their robust futexes each in its own exit, once it
/// stopped running the old image, as their `do_exit` does before `de_thread` sees them gone: a
/// lock one took after the `execve` killed it goes to its waiters by then.
#[test]
fn the_threads_an_execve_ends_release_their_robust_futexes_as_they_go() {
    let (kernel, leader, memory) = machine();
    let caller = Caller::new(leader, &memory);
    install_prog(&kernel, &caller, &memory);
    let other = spawn_thread(&kernel, &caller);
    let other_caller = Caller::new(other, &memory);
    while kernel.take_pending(leader).is_some() {}
    // `other`'s robust list: the head, empty, with the word 4 bytes into each entry.
    const HEAD: u32 = 0x3000;
    const ENTRY: u32 = 0x3010;
    put(&memory, HEAD, &HEAD.to_le_bytes());
    put(&memory, HEAD + 4, &4u32.to_le_bytes());
    let head = wasixv2_abi::RobustListHeadPtr::new(HEAD);
    assert_eq!(
        block_on(kernel.set_robust_list(&other_caller, head, 12)),
        Ok(0)
    );
    let mut exec = Box::pin(execve(&kernel, &caller));
    assert_eq!(exec.as_mut().now_or_never(), None);

    // Killed, `other` takes a mutex before it notices.
    put(&memory, ENTRY, &HEAD.to_le_bytes());
    put(&memory, ENTRY + 4, &other.0.to_le_bytes());
    put(&memory, HEAD, &ENTRY.to_le_bytes());
    assert_eq!(kernel.take_pending(other), Some(GuestAction::Exit(0)));
    block_on(kernel.exit_mm(&other_caller));
    assert_eq!(word(&memory, ENTRY + 4), 0x4000_0000, "FUTEX_OWNER_DIED");
    kernel.release(other);
    assert_eq!(exec.as_mut().now_or_never(), Some(Ok(0)));
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
    assert!(kernel.process(leader).exe.lock_or_recover().is_none());
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

/// A leader waiting for its vfork child while another thread calls `execve`: Linux's
/// `de_thread` kills it in `wait_for_vfork_done` (`zap_other_threads`), so it never returns
/// from vfork. When the child ends before the leader's instance left the leader's memory for
/// it, the instance goes back to the leader without resuming it, and the leader takes the new
/// image there.
#[test]
fn a_vfork_parent_handed_another_threads_image_does_not_return_from_vfork() {
    const VFORK: u32 = 0x4000 | 0x100 | Signal::SIGCHLD.0 as u32;
    let (kernel, leader, memory) = machine();
    let caller = Caller::new(leader, &memory);
    install_prog(&kernel, &caller, &memory);
    let execing = spawn_thread(&kernel, &caller);
    while kernel.take_pending(leader).is_some() {}
    let vfork = kernel.clone(&caller, VFORK, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL);
    assert_eq!(block_on(vfork), Ok(0));
    let Some(GuestAction::SwitchTask { task: child, .. }) = kernel.take_pending(leader) else {
        panic!("vfork left no task switch");
    };
    let thread = Caller::new(execing, &memory);
    let mut exec = Box::pin(execve(&kernel, &thread));
    assert_eq!(exec.as_mut().now_or_never(), None);

    // The child ends, and the instance goes back to the leader, which does not resume.
    block_on(kernel.exit_group(&Caller::new(child, &memory), 0)).unwrap();
    let parent = VforkParent {
        task: leader,
        resumes: Resume::Dies,
        pid: child.0 as i32,
    };
    let back = GuestAction::VforkReturn { parent, child };
    assert_eq!(kernel.take_pending(child), Some(back));
    kernel.release(child);
    // Its next work is the new image.
    let handed = GuestAction::Exec { vfork_parent: None };
    assert_eq!(kernel.take_pending(leader), Some(handed));
    let image = kernel.take_image(leader);
    assert_eq!(image.as_ref().map(Image::bytes), Some(IMAGE));
    let mut wait = Box::pin(kernel.exec_wait(leader));
    assert_eq!(wait.as_mut().now_or_never(), None);
    assert_eq!(exec.as_mut().now_or_never(), Some(Ok(0)));
    assert_eq!(kernel.take_pending(execing), Some(GuestAction::Exit(0)));
    kernel.release(execing);
    assert_eq!(wait.as_mut().now_or_never(), Some(Ok(())));
}

/// A process whose leader waits for its vfork child, which its instance runs, while another
/// thread of it is in `execve` of `/prog`.
struct LeaderInVfork {
    kernel: Kernel,
    memory: FlatMemory,
    leader: TaskId,
    execing: TaskId,
    child: TaskId,
}

impl LeaderInVfork {
    /// Returns the setup and the pending `execve`'s future.
    fn new() -> Self {
        const VFORK: u32 = 0x4000 | 0x100 | Signal::SIGCHLD.0 as u32;
        let (kernel, leader, memory) = machine();
        let caller = Caller::new(leader, &memory);
        install_prog(&kernel, &caller, &memory);
        let execing = spawn_thread(&kernel, &caller);
        while kernel.take_pending(leader).is_some() {}
        let vfork = kernel.clone(&caller, VFORK, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL);
        assert_eq!(block_on(vfork), Ok(0));
        let Some(GuestAction::SwitchTask { task: child, .. }) = kernel.take_pending(leader) else {
            panic!("vfork left no task switch");
        };
        kernel.work_flag(child).clear();
        Self {
            kernel,
            memory,
            leader,
            execing,
            child,
        }
    }

    fn caller(&self, task: TaskId) -> Caller<'_> {
        Caller::new(task, &self.memory)
    }

    /// The child's instance takes the leader's `exit_mm` and runs it.
    fn leave_leaders_memory(&self) {
        let dead = GuestAction::ExitMm { task: self.leader };
        assert_eq!(self.kernel.take_pending(self.child), Some(dead));
        block_on(self.kernel.exit_mm(&self.caller(self.leader)));
    }
}

/// Linux's `de_thread` does not wait for the vfork child of a leader it kills in
/// `wait_for_vfork_done`, only for the leader's own exit. So an `execve` from another thread
/// commits once the child's instance left the leader's memory for it, and the exec'ing thread
/// starts the new image as the leader in an instance of its own before it exits; the new image
/// starts once that thread is gone. The leader's old instance runs the child on, and ends with
/// it.
#[test]
fn a_sibling_execve_does_not_wait_for_the_leaders_vfork_child() {
    let t = LeaderInVfork::new();
    let (kernel, leader, execing, child) = (&t.kernel, t.leader, t.execing, t.child);
    let thread = t.caller(execing);
    let mut exec = Box::pin(execve(kernel, &thread));
    assert_eq!(exec.as_mut().now_or_never(), None);
    assert!(
        kernel.work_flag(child).is_raised(),
        "the child leaves its wait"
    );
    t.leave_leaders_memory();
    assert_eq!(exec.as_mut().now_or_never(), Some(Ok(0)));

    let start = GuestAction::StartImage { task: leader };
    assert_eq!(kernel.take_pending(execing), Some(start));
    let image = kernel.take_image(leader);
    assert_eq!(image.as_ref().map(Image::bytes), Some(IMAGE));
    assert_eq!(kernel.take_pending(execing), Some(GuestAction::Exit(0)));
    let mut wait = Box::pin(kernel.exec_wait(leader));
    assert_eq!(
        wait.as_mut().now_or_never(),
        None,
        "the exec'ing thread is there"
    );
    kernel.release(execing);
    assert_eq!(wait.as_mut().now_or_never(), Some(Ok(())));
    assert_eq!(kernel.take_pending(leader), None, "no exec is left to take");

    // The child runs on; its end ends the old instance, which never runs as the leader again.
    assert_eq!(kernel.take_pending(child), None);
    block_on(kernel.exit_group(&t.caller(child), 0)).unwrap();
    let parent = VforkParent {
        task: leader,
        resumes: Resume::Gone,
        pid: child.0 as i32,
    };
    let back = GuestAction::VforkReturn { parent, child };
    assert_eq!(kernel.take_pending(child), Some(back));
}

/// The same for a child that `execve`s after the leader's image went on: its new image runs on
/// its own, and the old instance ends.
#[test]
fn a_vfork_child_execve_after_its_parent_went_on_ends_the_old_instance() {
    let t = LeaderInVfork::new();
    let (kernel, leader, execing, child) = (&t.kernel, t.leader, t.execing, t.child);
    let thread = t.caller(execing);
    let mut exec = Box::pin(execve(kernel, &thread));
    assert_eq!(exec.as_mut().now_or_never(), None);
    t.leave_leaders_memory();
    assert_eq!(exec.as_mut().now_or_never(), Some(Ok(0)));

    assert_eq!(block_on(execve(kernel, &t.caller(child))), Ok(0));
    let vfork_parent = Some(VforkParent {
        task: leader,
        resumes: Resume::Gone,
        pid: child.0 as i32,
    });
    let exec = GuestAction::Exec { vfork_parent };
    assert_eq!(kernel.take_pending(child), Some(exec));
}

/// A child that ends after its instance left the leader's memory, but before the `execve`
/// committed, still hands the instance back to the leader, which does not return from vfork
/// but takes the new image there: the `execve` then waits for that, as for any leader.
#[test]
fn a_vfork_child_that_ends_before_the_sibling_execve_commits_hands_the_leader_back() {
    let t = LeaderInVfork::new();
    let (kernel, leader, execing, child) = (&t.kernel, t.leader, t.execing, t.child);
    let thread = t.caller(execing);
    let mut exec = Box::pin(execve(kernel, &thread));
    assert_eq!(exec.as_mut().now_or_never(), None);
    t.leave_leaders_memory();
    block_on(kernel.exit_group(&t.caller(child), 0)).unwrap();
    assert_eq!(
        exec.as_mut().now_or_never(),
        None,
        "the leader waits no more"
    );

    let parent = VforkParent {
        task: leader,
        resumes: Resume::Dies,
        pid: child.0 as i32,
    };
    let back = GuestAction::VforkReturn { parent, child };
    assert_eq!(kernel.take_pending(child), Some(back));
    kernel.release(child);
    let handed = GuestAction::Exec { vfork_parent: None };
    assert_eq!(kernel.take_pending(leader), Some(handed));
    let mut wait = Box::pin(kernel.exec_wait(leader));
    assert_eq!(wait.as_mut().now_or_never(), None);
    assert_eq!(exec.as_mut().now_or_never(), Some(Ok(0)));
    assert_eq!(kernel.take_pending(execing), Some(GuestAction::Exit(0)));
    kernel.release(execing);
    assert_eq!(wait.as_mut().now_or_never(), Some(Ok(())));
}

/// A thread that is not the leader, waiting for its vfork child, which its instance runs, while
/// another task of its group is in `execve` of `/prog`.
struct ThreadInVfork {
    kernel: Kernel,
    memory: FlatMemory,
    leader: TaskId,
    thread: TaskId,
    child: TaskId,
}

impl ThreadInVfork {
    fn new() -> Self {
        const VFORK: u32 = 0x4000 | 0x100 | Signal::SIGCHLD.0 as u32;
        let (kernel, leader, memory) = machine();
        let caller = Caller::new(leader, &memory);
        install_prog(&kernel, &caller, &memory);
        let thread = spawn_thread(&kernel, &caller);
        while kernel.take_pending(leader).is_some() {}
        let parent = Caller::new(thread, &memory);
        let vfork = kernel.clone(&parent, VFORK, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL);
        assert_eq!(block_on(vfork), Ok(0));
        let Some(GuestAction::SwitchTask { task: child, .. }) = kernel.take_pending(thread) else {
            panic!("vfork left no task switch");
        };
        kernel.work_flag(child).clear();
        Self {
            kernel,
            memory,
            leader,
            thread,
            child,
        }
    }

    fn caller(&self, task: TaskId) -> Caller<'_> {
        Caller::new(task, &self.memory)
    }

    /// The `execve` killed the thread: the child's instance takes the thread's `exit_mm` and
    /// runs it.
    fn leave_threads_memory(&self) {
        assert!(
            self.kernel.work_flag(self.child).is_raised(),
            "the child leaves its wait"
        );
        let dead = GuestAction::ExitMm { task: self.thread };
        assert_eq!(self.kernel.take_pending(self.child), Some(dead));
        block_on(self.kernel.exit_mm(&self.caller(self.thread)));
    }

    /// The child ends: the instance goes back to the thread, which does not return from vfork
    /// but unwinds through its exit, and goes.
    fn end_child(&self) {
        let (kernel, thread, child) = (&self.kernel, self.thread, self.child);
        block_on(kernel.exit_group(&self.caller(child), 0)).unwrap();
        let parent = VforkParent {
            task: thread,
            resumes: Resume::Dies,
            pid: child.0 as i32,
        };
        let back = GuestAction::VforkReturn { parent, child };
        assert_eq!(kernel.take_pending(child), Some(back));
        kernel.release(child);
        assert_eq!(kernel.take_pending(thread), Some(GuestAction::Exit(0)));
        kernel.release(thread);
        assert!(!live(kernel, thread));
    }
}

/// `zap_other_threads` kills a thread waiting in `wait_for_vfork_done`, which is killable, and
/// `de_thread` waits only for it to exit, not for its vfork child: the leader's `execve`
/// commits once the child's instance left the thread's memory for it, and the new image starts
/// while the child runs on.
#[test]
fn an_execve_does_not_wait_for_a_threads_vfork_child() {
    let t = ThreadInVfork::new();
    let (kernel, leader) = (&t.kernel, t.leader);
    let caller = t.caller(leader);
    let mut exec = Box::pin(execve(kernel, &caller));
    assert_eq!(exec.as_mut().now_or_never(), None);
    t.leave_threads_memory();
    assert_eq!(exec.as_mut().now_or_never(), Some(Ok(0)));
    assert!(kernel.process(leader).exe.lock().unwrap().is_some());
    let handed = GuestAction::Exec { vfork_parent: None };
    assert_eq!(kernel.take_pending(leader), Some(handed));
    let mut wait = Box::pin(kernel.exec_wait(leader));
    assert_eq!(wait.as_mut().now_or_never(), Some(Ok(())));
    t.end_child();
    assert_eq!(kernel.take_pending(leader), None);
}

/// The same for an `execve` from a third thread: the leader takes the new image, which starts
/// once the exec'ing thread is gone, whatever the vfork child does.
#[test]
fn a_sibling_execve_does_not_wait_for_another_threads_vfork_child() {
    let t = ThreadInVfork::new();
    let (kernel, leader) = (&t.kernel, t.leader);
    let execing = spawn_thread(kernel, &t.caller(leader));
    while kernel.take_pending(leader).is_some() {}
    let caller = t.caller(execing);
    let mut exec = Box::pin(execve(kernel, &caller));
    assert_eq!(exec.as_mut().now_or_never(), None);
    let handed = GuestAction::Exec { vfork_parent: None };
    assert_eq!(kernel.take_pending(leader), Some(handed));
    let mut wait = Box::pin(kernel.exec_wait(leader));
    assert_eq!(wait.as_mut().now_or_never(), None);
    t.leave_threads_memory();
    assert_eq!(exec.as_mut().now_or_never(), Some(Ok(0)));
    assert_eq!(kernel.take_pending(execing), Some(GuestAction::Exit(0)));
    assert_eq!(wait.as_mut().now_or_never(), None);
    kernel.release(execing);
    assert_eq!(wait.as_mut().now_or_never(), Some(Ok(())));
    t.end_child();
}

/// Until the child's instance left the thread's memory, the thread still runs the old image as
/// far as `de_thread` knows, so the `execve` waits; a child that ends first hands the instance
/// back to the thread, which unwinds and goes, and the `execve` commits then.
#[test]
fn an_execve_waits_for_a_vfork_parent_thread_until_it_left_its_memory() {
    let t = ThreadInVfork::new();
    let (kernel, leader, thread, child) = (&t.kernel, t.leader, t.thread, t.child);
    let caller = t.caller(leader);
    let mut exec = Box::pin(execve(kernel, &caller));
    assert_eq!(exec.as_mut().now_or_never(), None);
    block_on(kernel.exit_group(&t.caller(child), 0)).unwrap();
    assert_eq!(exec.as_mut().now_or_never(), None);
    let parent = VforkParent {
        task: thread,
        resumes: Resume::Dies,
        pid: child.0 as i32,
    };
    let back = GuestAction::VforkReturn { parent, child };
    assert_eq!(kernel.take_pending(child), Some(back));
    kernel.release(child);
    assert_eq!(kernel.take_pending(thread), Some(GuestAction::Exit(0)));
    assert_eq!(exec.as_mut().now_or_never(), None);
    block_on(kernel.exit_mm(&t.caller(thread)));
    kernel.release(thread);
    assert_eq!(exec.as_mut().now_or_never(), Some(Ok(0)));
}

/// A leader that is itself a vfork child takes the new image in its instance, where its own
/// vfork parent waits to resume: no other instance may start the image, which would lose
/// that resume. Its `execve` from another thread so waits for its own vfork child, as
/// before.
#[test]
fn a_leader_that_is_a_vfork_child_takes_the_image_in_its_instance() {
    const VFORK: u32 = 0x4000 | 0x100 | Signal::SIGCHLD.0 as u32;
    let (kernel, grandparent, memory) = machine();
    let caller = Caller::new(grandparent, &memory);
    install_prog(&kernel, &caller, &memory);
    let vfork = |caller: &Caller<'_>| {
        let clone = kernel.clone(caller, VFORK, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL);
        assert_eq!(block_on(clone), Ok(0));
        match kernel.take_pending(caller.task()) {
            Some(GuestAction::SwitchTask { task, .. }) => task,
            other => panic!("vfork left no task switch: {other:?}"),
        }
    };
    let leader = vfork(&caller);
    let leader_caller = Caller::new(leader, &memory);
    let execing = spawn_thread(&kernel, &leader_caller);
    while kernel.take_pending(leader).is_some() {}
    let child = vfork(&leader_caller);
    let thread = Caller::new(execing, &memory);
    let mut exec = Box::pin(execve(&kernel, &thread));
    assert_eq!(exec.as_mut().now_or_never(), None);
    let dead = GuestAction::ExitMm { task: leader };
    assert_eq!(kernel.take_pending(child), Some(dead));
    block_on(kernel.exit_mm(&leader_caller));
    assert_eq!(
        exec.as_mut().now_or_never(),
        None,
        "waits for the leader's child"
    );

    block_on(kernel.exit_group(&Caller::new(child, &memory), 0)).unwrap();
    let parent = VforkParent {
        task: leader,
        resumes: Resume::Dies,
        pid: child.0 as i32,
    };
    let back = GuestAction::VforkReturn { parent, child };
    assert_eq!(kernel.take_pending(child), Some(back));
    kernel.release(child);
    let vfork_parent = Some(VforkParent {
        task: grandparent,
        resumes: Resume::Returns,
        pid: leader.0 as i32,
    });
    let handed = GuestAction::Exec { vfork_parent };
    assert_eq!(kernel.take_pending(leader), Some(handed));
    let mut wait = Box::pin(kernel.exec_wait(leader));
    assert_eq!(wait.as_mut().now_or_never(), None);
    assert_eq!(exec.as_mut().now_or_never(), Some(Ok(0)));
    assert_eq!(kernel.take_pending(execing), Some(GuestAction::Exit(0)));
    kernel.release(execing);
    assert_eq!(wait.as_mut().now_or_never(), Some(Ok(())));
}

/// A fatal signal, or another thread's `exit_group`, while the `execve` waits for the leader:
/// the `execve` fails, the leader's instance still leaves the leader's memory for it, and the
/// child's end hands the instance back to the dead leader.
#[test]
fn a_group_that_dies_while_a_sibling_execve_waits_for_the_vfork_leader_execs_nothing() {
    for case in ["SIGKILL", "exit_group"] {
        let t = LeaderInVfork::new();
        let (kernel, leader, execing, child) = (&t.kernel, t.leader, t.execing, t.child);
        let other = spawn_thread(kernel, &t.caller(execing));
        while kernel.take_pending(execing).is_some() {}
        let thread = t.caller(execing);
        let mut exec = Box::pin(execve(kernel, &thread));
        assert_eq!(exec.as_mut().now_or_never(), None, "{case}");
        match case {
            "SIGKILL" => {
                let kill = kernel.sys_kill(&thread, leader.0 as Pid, Signal::SIGKILL);
                assert_eq!(block_on(kill), Ok(0));
            }
            _ => block_on(kernel.exit_group(&t.caller(other), 9)).unwrap(),
        }
        let failed = exec.as_mut().now_or_never();
        assert_eq!(failed, Some(Err(Errno::EAGAIN.into())), "{case}");
        assert!(
            matches!(kernel.take_pending(execing), Some(GuestAction::Exit(_))),
            "{case}"
        );
        t.leave_leaders_memory();

        block_on(kernel.exit_group(&t.caller(child), 0)).unwrap();
        let parent = VforkParent {
            task: leader,
            resumes: Resume::Dies,
            pid: child.0 as i32,
        };
        let back = GuestAction::VforkReturn { parent, child };
        assert_eq!(kernel.take_pending(child), Some(back), "{case}");
    }
}

const IMAGE: &[u8] = b"\0asm\x01\0\0\0";

/// Writes an executable `/prog` holding [`IMAGE`], and `execve`'s arguments for it: the path
/// at 0x2000, argv (`prog`) at 0x2010, an empty envp at 0x2018.
pub(super) fn install_prog(kernel: &Kernel, caller: &Caller<'_>, memory: &FlatMemory) {
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
pub(super) fn execve<'a>(
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
pub(super) fn handle(kernel: &Kernel, caller: &Caller<'_>, sig: Signal) {
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
    assert!(kernel.process(leader).exe.lock_or_recover().is_none());
    assert_eq!(kernel.take_pending(thread), Some(GuestAction::Exit(3)));
    assert_eq!(kernel.take_pending(thread), None);
    assert_eq!(kernel.take_pending(leader), Some(GuestAction::Exit(3)));
    assert_eq!(kernel.take_pending(leader), None);
}

/// argv and envp are bytes: strings that are not UTF-8 reach the new image unchanged, through
/// a script's `#!` line too.
#[test]
fn execve_keeps_arguments_and_environment_as_bytes() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    install_prog(&kernel, &caller, &memory);
    put(&memory, 0x2200, b"/script\0");
    put(&memory, 0x2300, b"#!/prog \xe9x\n");
    let create = OpenFlags::O_CREAT | OpenFlags::O_WRONLY;
    let open = kernel.openat(&caller, Fd(-100), GuestPtr::new(0x2200), create, 0o755);
    let fd = block_on(open).unwrap();
    let write = kernel.write(&caller, fd, UserSlice::new(0x2300, 11));
    assert_eq!(block_on(write), Ok(11));
    assert_eq!(block_on(kernel.close(&caller, fd)), Ok(0));
    put(&memory, 0x2400, b"caf\xe9\0X=\xff\0");
    for (at, words) in [(0x2410, [0x2200, 0x2400, 0]), (0x2420, [0x2405, 0, 0])] {
        let bytes: Vec<u8> = words.iter().flat_map(|w: &u32| w.to_le_bytes()).collect();
        put(&memory, at, &bytes);
    }

    let exec = kernel.execve(
        &caller,
        GuestPtr::new(0x2200),
        GuestPtr::new(0x2410),
        GuestPtr::new(0x2420),
    );
    assert_eq!(block_on(exec), Ok(0));

    let process = kernel.process(task);
    let args: &[&[u8]] = &[b"/prog", b"\xe9x", b"/script", b"caf\xe9"];
    assert_eq!(*process.args.lock_or_recover(), args);
    assert_eq!(*process.env.lock_or_recover(), [b"X=\xff"]);
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

/// `rseq` registers one area per thread, checked against its length and signature, and fills
/// in the ids at once: CPU 0, node 0, concurrency id 0.
#[test]
fn rseq_registers_one_area_per_thread() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let rseq = |area: u32, len: u32, flags: i32, sig: u32| {
        block_on(kernel.rseq(&caller, GuestPtrMut::new(area), len, flags, sig))
    };
    const AREA: u32 = 0x1040;
    const SIG: u32 = 0x5305_3053;
    super::ipc::put(&memory, AREA, &[0xff; 64]);
    assert_eq!(errno(rseq(AREA + 16, 32, 0, SIG)), Errno::EINVAL);
    assert_eq!(errno(rseq(AREA + 32, 64, 0, SIG)), Errno::EINVAL);
    assert_eq!(errno(rseq(AREA, 31, 0, SIG)), Errno::EINVAL);
    assert_eq!(errno(rseq(AREA, 64, 4, SIG)), Errno::EINVAL);
    assert_eq!(rseq(AREA, 64, 0, SIG), Ok(0));
    // cpu_id_start, cpu_id, rseq_cs, flags, node_id, mm_cid; the rest is untouched.
    let area = super::ipc::get(&memory, AREA, 32);
    assert_eq!(area[..28], [0; 28]);
    assert_eq!(area[28..32], [0xff; 4]);

    assert_eq!(errno(rseq(AREA, 64, 0, SIG)), Errno::EBUSY);
    assert_eq!(errno(rseq(AREA, 64, 0, SIG + 1)), Errno::EPERM);
    assert_eq!(errno(rseq(AREA + 64, 64, 0, SIG)), Errno::EINVAL);

    // A new thread has none; registering its own is fine.
    let thread = spawn_thread(&kernel, &caller);
    let thread_caller = Caller::new(thread, &memory);
    let other = kernel.rseq(&thread_caller, GuestPtrMut::new(AREA + 0x40), 32, 0, SIG);
    assert_eq!(block_on(other), Ok(0));

    let unregister = |len: u32, flags: i32, sig: u32| rseq(AREA, len, 1 | flags, sig);
    assert_eq!(errno(unregister(32, 0, SIG)), Errno::EINVAL);
    assert_eq!(errno(unregister(64, 2, SIG)), Errno::EINVAL);
    assert_eq!(errno(unregister(64, 0, SIG + 1)), Errno::EPERM);
    assert_eq!(unregister(64, 0, SIG), Ok(0));
    assert_eq!(super::word(&memory, AREA + 4), u32::MAX);
    assert_eq!(errno(unregister(64, 0, SIG)), Errno::EINVAL);
    assert_eq!(rseq(AREA, 32, 0, SIG), Ok(0));
}

/// A fork child keeps its parent's rseq area; a child on the same memory gets none, and an
/// `execve` drops it (`rseq_fork`, `rseq_execve`).
#[test]
fn rseq_areas_follow_fork_but_not_clone_vm() {
    let mut parent = thread::ThreadState::default();
    parent.rseq = Some(crate::rseq::Rseq {
        area: 0x1000,
        len: 32,
        sig: 1,
    });
    let fork = parent.for_process(CloneFlags(Signal::SIGCHLD.0 as u64), 0, 0);
    assert_eq!(fork.rseq, parent.rseq);
    let vm = parent.for_process(CloneFlags::CLONE_VM | CloneFlags::CLONE_VFORK, 0, 0);
    assert_eq!(vm.rseq, None);
}

/// binfmt_misc: a registered format's files run through its interpreter, which gets the file's
/// name, before any `#!` line is looked at; formats are shown, turned off and removed through
/// their files.
#[test]
fn binfmt_misc_runs_registered_formats_through_their_interpreter() {
    let (kernel, task, memory) = super::cred::machine_as(0, 0);
    let caller = Caller::new(task, &memory);
    install_prog(&kernel, &caller, &memory);
    let cstr = |addr: u32, s: &[u8]| {
        let mut bytes = s.to_vec();
        bytes.push(0);
        put(&memory, addr, &bytes);
        Cstr::new(addr)
    };
    let mkdir = kernel.mkdirat(&caller, Fd(-100), cstr(0x3000, b"/bm"), 0o755);
    assert_eq!(block_on(mkdir), Ok(0));
    let mount = kernel.mount(
        &caller,
        cstr(0x3000, b"binfmt_misc"),
        cstr(0x3100, b"/bm"),
        cstr(0x3200, b"binfmt_misc"),
        wasixv2_abi::MountFlags(0),
        wasixv2_abi::ConstVoidPtr::NULL,
    );
    assert_eq!(block_on(mount), Ok(0));
    let write = |path: &[u8], data: &[u8]| {
        let open = kernel.openat(
            &caller,
            Fd(-100),
            cstr(0x3000, path),
            OpenFlags::O_WRONLY,
            0,
        );
        let fd = block_on(open)?;
        put(&memory, 0x3400, data);
        let n = block_on(kernel.write(&caller, fd, UserSlice::new(0x3400, data.len() as u32)));
        assert_eq!(block_on(kernel.close(&caller, fd)), Ok(0));
        n
    };
    let read = |path: &[u8]| {
        let open = kernel.openat(
            &caller,
            Fd(-100),
            cstr(0x3000, path),
            OpenFlags::O_RDONLY,
            0,
        );
        let fd = block_on(open)?;
        let n = block_on(kernel.read(&caller, fd, UserSliceMut::new(0x3800, 512)))?;
        assert_eq!(block_on(kernel.close(&caller, fd)), Ok(0));
        Ok::<_, SysErr>(memory.0.borrow()[0x3800..0x3800 + n as usize].to_vec())
    };
    let create = |path: &[u8], data: &[u8]| {
        let flags = OpenFlags::O_CREAT | OpenFlags::O_WRONLY;
        let open = kernel.openat(&caller, Fd(-100), cstr(0x3000, path), flags, 0o755);
        let fd = block_on(open).unwrap();
        put(&memory, 0x3400, data);
        let write = kernel.write(&caller, fd, UserSlice::new(0x3400, data.len() as u32));
        assert_eq!(block_on(write), Ok(data.len() as u32));
        assert_eq!(block_on(kernel.close(&caller, fd)), Ok(0));
    };
    let exec = |path: &[u8], argv: &[&[u8]]| {
        cstr(0x2200, path);
        let mut ptrs = Vec::new();
        let mut at = 0x2600;
        for arg in argv {
            cstr(at, arg);
            ptrs.extend_from_slice(&at.to_le_bytes());
            at += 0x40;
        }
        ptrs.extend_from_slice(&[0; 4]);
        put(&memory, 0x2500, &ptrs);
        put(&memory, 0x2580, &[0; 4]);
        let call = kernel.execve(
            &caller,
            GuestPtr::new(0x2200),
            GuestPtr::new(0x2500),
            GuestPtr::new(0x2580),
        );
        block_on(call)
    };
    let args = || kernel.process(task).args.lock_or_recover().clone();

    assert_eq!(read(b"/bm/status"), Ok(b"enabled\n".to_vec()));
    let demo = b":demo:M::DEMO::/prog:\n";
    assert_eq!(write(b"/bm/register", demo), Ok(demo.len() as u32));
    assert_eq!(errno(write(b"/bm/register", demo)), Errno::EEXIST);
    let shown = b"enabled\ninterpreter /prog\nflags: \noffset 0\nmagic 44454d4f\n";
    assert_eq!(read(b"/bm/demo"), Ok(shown.to_vec()));

    // A magic match: the interpreter, the file's name, then argv[1] on.
    create(b"/data", b"DEMOxyz");
    assert_eq!(exec(b"/data", &[b"data", b"a"]), Ok(0));
    assert_eq!(args(), [&b"/prog"[..], b"/data", b"a"]);
    // An extension match keeping argv[0] (`P`).
    let ext = b":ext:E::dx::/prog:P";
    assert_eq!(write(b"/bm/register", ext), Ok(ext.len() as u32));
    create(b"/f.dx", b"zz");
    assert_eq!(exec(b"/f.dx", &[b"zero", b"one"]), Ok(0));
    assert_eq!(args(), [&b"/prog"[..], b"/f.dx", b"zero", b"one"]);

    // Turned off, the format matches nothing, and the file is no executable at all.
    assert_eq!(write(b"/bm/demo", b"0"), Ok(1));
    assert_eq!(errno(exec(b"/data", &[b"data"])), Errno::ENOEXEC);
    assert_eq!(write(b"/bm/demo", b"1"), Ok(1));
    assert_eq!(write(b"/bm/status", b"0\n"), Ok(2));
    assert_eq!(errno(exec(b"/data", &[b"data"])), Errno::ENOEXEC);
    assert_eq!(write(b"/bm/status", b"1"), Ok(1));
    // Removed one at a time, or all at once.
    assert_eq!(write(b"/bm/demo", b"-1"), Ok(2));
    assert_eq!(errno(read(b"/bm/demo")), Errno::ENOENT);
    assert_eq!(write(b"/bm/status", b"-1"), Ok(2));
    assert_eq!(errno(read(b"/bm/ext")), Errno::ENOENT);
    assert_eq!(
        errno(write(b"/bm/register", b":bad:M::x::/i:O")),
        Errno::EINVAL
    );
}

#[test]
fn rseq_slice_yield_has_no_extension_to_relinquish() {
    use wasixv2_abi::{PrctlArg, PrctlOption, RseqFlags};
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    assert_eq!(block_on(kernel.rseq_slice_yield(&caller)), Ok(0));
    assert_eq!(
        block_on(kernel.rseq(
            &caller,
            GuestPtrMut::new(0x1040),
            64,
            RseqFlags::RSEQ_FLAG_SLICE_EXT_DEFAULT_ON.0,
            0x53053053
        )),
        Ok(0)
    );
    for op in [
        PrctlArg::PR_RSEQ_SLICE_EXTENSION_GET,
        PrctlArg::PR_RSEQ_SLICE_EXTENSION_SET,
    ] {
        assert_eq!(
            errno(block_on(kernel.prctl(
                &caller,
                PrctlOption::PR_RSEQ_SLICE_EXTENSION,
                op.0,
                1,
                0,
                0
            ))),
            Errno::EINVAL
        );
    }
    assert_eq!(block_on(kernel.rseq_slice_yield(&caller)), Ok(0));
    assert_eq!(block_on(kernel.rseq_slice_yield(&caller)), Ok(0));
    assert_eq!(
        super::ipc::get(&memory, 0x1050, 4),
        [0; 4],
        "no feature flags enabled"
    );
}

#[test]
fn prctl_syscall_user_dispatch_and_config_disabled_controls_are_einval() {
    use wasixv2_abi::PrctlOption as P;
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    for mode in [0, 1, 2, u32::MAX] {
        assert_eq!(
            errno(block_on(kernel.prctl(
                &caller,
                P::PR_SET_SYSCALL_USER_DISPATCH,
                mode,
                0x1000,
                0x100,
                0xfffffff0
            ))),
            Errno::EINVAL
        );
    }
    for op in [
        P::PR_GET_UNALIGN,
        P::PR_SET_UNALIGN,
        P::PR_GET_FPEMU,
        P::PR_SET_FPEMU,
        P::PR_GET_FPEXC,
        P::PR_SET_FPEXC,
        P::PR_GET_ENDIAN,
        P::PR_SET_ENDIAN,
        P::PR_GET_TSC,
        P::PR_SET_TSC,
        P::PR_GET_FP_MODE,
        P::PR_SET_FP_MODE,
        P::PR_SVE_GET_VL,
        P::PR_SVE_SET_VL,
        P::PR_SME_GET_VL,
        P::PR_SME_SET_VL,
        P::PR_PAC_RESET_KEYS,
        P::PR_PAC_GET_ENABLED_KEYS,
        P::PR_PAC_SET_ENABLED_KEYS,
        P::PR_GET_TAGGED_ADDR_CTRL,
        P::PR_SET_TAGGED_ADDR_CTRL,
        P::PR_GET_SPECULATION_CTRL,
        P::PR_SET_SPECULATION_CTRL,
        P::PR_SCHED_CORE,
        P::PR_GET_MEMORY_MERGE,
        P::PR_SET_MEMORY_MERGE,
        P::PR_GET_SHADOW_STACK_STATUS,
        P::PR_SET_SHADOW_STACK_STATUS,
        P::PR_LOCK_SHADOW_STACK_STATUS,
        P::PR_GET_CFI,
        P::PR_SET_CFI,
        P::PR_RISCV_V_GET_CONTROL,
        P::PR_RISCV_V_SET_CONTROL,
        P::PR_RISCV_SET_ICACHE_FLUSH_CTX,
        P::PR_PPC_GET_DEXCR,
        P::PR_PPC_SET_DEXCR,
    ] {
        assert_eq!(
            errno(block_on(kernel.prctl(&caller, op, 0, 0, 0, 0))),
            Errno::EINVAL,
            "{op:?}"
        );
    }
}

#[test]
fn prctl_io_flusher_and_mce_policy_are_per_task_and_inherited() {
    use wasixv2_abi::PrctlOption as P;
    let (kernel, task, memory) = super::cred::machine_as(0, 0);
    let caller = Caller::new(task, &memory);
    let pr = |caller: &Caller<'_>, op, a, b| block_on(kernel.prctl(caller, op, a, b, 0, 0));
    assert_eq!(pr(&caller, P::PR_GET_IO_FLUSHER, 0, 0), Ok(0));
    assert_eq!(pr(&caller, P::PR_SET_IO_FLUSHER, 1, 0), Ok(0));
    assert_eq!(pr(&caller, P::PR_MCE_KILL_GET, 0, 0), Ok(2));
    assert_eq!(pr(&caller, P::PR_MCE_KILL, 1, 1), Ok(0));
    let thread = spawn_thread(&kernel, &caller);
    let other = Caller::new(thread, &memory);
    assert_eq!(pr(&other, P::PR_GET_IO_FLUSHER, 0, 0), Ok(1));
    assert_eq!(pr(&other, P::PR_MCE_KILL_GET, 0, 0), Ok(1));
    assert_eq!(pr(&other, P::PR_SET_IO_FLUSHER, 0, 0), Ok(0));
    assert_eq!(pr(&other, P::PR_MCE_KILL, 1, 0), Ok(0));
    assert_eq!(pr(&caller, P::PR_GET_IO_FLUSHER, 0, 0), Ok(1));
    assert_eq!(pr(&caller, P::PR_MCE_KILL_GET, 0, 0), Ok(1));
    assert_eq!(
        errno(pr(&caller, P::PR_SET_IO_FLUSHER, 2, 0)),
        Errno::EINVAL
    );
    assert_eq!(
        errno(pr(&caller, P::PR_GET_IO_FLUSHER, 1, 0)),
        Errno::EINVAL
    );
    assert_eq!(pr(&other, P::PR_MCE_KILL, 0, 0), Ok(0));
    assert_eq!(errno(pr(&other, P::PR_MCE_KILL, 1, 99)), Errno::EINVAL);
    assert_eq!(
        pr(&other, P::PR_MCE_KILL_GET, 0, 0),
        Ok(0),
        "Linux sets PF_MCE_PROCESS before rejecting arg3"
    );
    assert_eq!(errno(pr(&other, P::PR_MCE_KILL, 0, 1)), Errno::EINVAL);
    super::cred::set_caps(&kernel, &caller, &memory, 0);
    assert_eq!(
        errno(pr(&caller, P::PR_SET_IO_FLUSHER, 99, 99)),
        Errno::EPERM
    );
    assert_eq!(errno(pr(&caller, P::PR_GET_IO_FLUSHER, 0, 0)), Errno::EPERM);
}

#[test]
fn prctl_auxv_returns_full_size_and_tid_address_is_per_thread() {
    use wasixv2_abi::PrctlOption as P;
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let auxv = [6u32.to_le_bytes(), 65536u32.to_le_bytes(), [0; 4], [0; 4]].concat();
    *kernel.process(task).auxv.lock_or_recover() = auxv.clone();
    let full = crate::startup::SAVED_AUXV_SIZE as i32;
    assert_eq!(
        block_on(kernel.prctl(&caller, P::PR_GET_AUXV, u32::MAX, 0, 0, 0)),
        Ok(full)
    );
    assert_eq!(
        block_on(kernel.prctl(&caller, P::PR_GET_AUXV, 0x400, 7, 0, 0)),
        Ok(full)
    );
    assert_eq!(super::ipc::get(&memory, 0x400, 7), auxv[..7]);
    assert_eq!(
        errno(block_on(kernel.prctl(
            &caller,
            P::PR_GET_AUXV,
            u32::MAX,
            1,
            0,
            0
        ))),
        Errno::EFAULT
    );
    assert_eq!(
        errno(block_on(kernel.prctl(
            &caller,
            P::PR_GET_AUXV,
            u32::MAX,
            1,
            1,
            0
        ))),
        Errno::EINVAL
    );
    block_on(kernel.set_tid_address(&caller, GuestPtrMut::new(0x1234))).unwrap();
    block_on(kernel.prctl(&caller, P::PR_GET_TID_ADDRESS, 0x400, 99, 99, 99)).unwrap();
    let addr: u32 = block_on(read_value(&memory, GuestPtr::new(0x400))).unwrap();
    assert_eq!(addr, 0x1234);
    let tid = spawn_thread(&kernel, &caller);
    block_on(kernel.prctl(
        &Caller::new(tid, &memory),
        P::PR_GET_TID_ADDRESS,
        0x400,
        0,
        0,
        0,
    ))
    .unwrap();
    let addr: u32 = block_on(read_value(&memory, GuestPtr::new(0x400))).unwrap();
    assert_eq!(addr, 0);
}

#[test]
fn prctl_mm_flags_share_with_threads_and_obey_fork_no_inherit() {
    use wasixv2_abi::PrctlOption as P;
    let (kernel, task, flat) = machine();
    let memory = super::proc::ForkableMemory(flat);
    let caller = Caller::new(task, &memory);
    let pr = |caller: &Caller<'_>, op, a, b| block_on(kernel.prctl(caller, op, a, b, 0, 0));
    assert_eq!(errno(pr(&caller, P::PR_SET_MDWE, 2, 0)), Errno::EINVAL);
    assert_eq!(errno(pr(&caller, P::PR_SET_MDWE, 4, 0)), Errno::EINVAL);
    assert_eq!(pr(&caller, P::PR_SET_MDWE, 3, 0), Ok(0));
    assert_eq!(errno(pr(&caller, P::PR_SET_MDWE, 1, 0)), Errno::EPERM);
    let tid = spawn_thread(&kernel, &caller);
    let other = Caller::new(tid, &memory);
    assert_eq!(pr(&other, P::PR_GET_MDWE, 0, 0), Ok(3));
    assert_eq!(pr(&other, P::PR_SET_THP_DISABLE, 1, 2), Ok(0));
    assert_eq!(pr(&caller, P::PR_GET_THP_DISABLE, 0, 0), Ok(3));
    assert_eq!(
        errno(pr(&caller, P::PR_SET_THP_DISABLE, 0, 2)),
        Errno::EINVAL
    );
    assert_eq!(pr(&caller, P::PR_FUTEX_HASH, 1, 4), Ok(0));
    assert_eq!(pr(&other, P::PR_FUTEX_HASH, 2, 99), Ok(4));
    let child = super::proc::fork_child(&kernel, &caller);
    let child_caller = Caller::new(child, &memory);
    assert_eq!(pr(&child_caller, P::PR_GET_MDWE, 0, 0), Ok(0));
    assert_eq!(pr(&child_caller, P::PR_GET_THP_DISABLE, 0, 0), Ok(3));
    assert_eq!(pr(&child_caller, P::PR_FUTEX_HASH, 2, 0), Ok(0));
    assert_eq!(pr(&child_caller, P::PR_SET_MDWE, 1, 0), Ok(0));
    let grandchild = super::proc::fork_child(&kernel, &child_caller);
    assert_eq!(
        pr(&Caller::new(grandchild, &memory), P::PR_GET_MDWE, 0, 0),
        Ok(1)
    );
    for slots in [1, 3, 7] {
        assert_eq!(
            errno(pr(&caller, P::PR_FUTEX_HASH, 1, slots)),
            Errno::EINVAL
        );
    }
    assert_eq!(pr(&caller, P::PR_FUTEX_HASH, 1, 0), Ok(0));
    assert_eq!(errno(pr(&caller, P::PR_FUTEX_HASH, 1, 4)), Errno::EBUSY);
}
