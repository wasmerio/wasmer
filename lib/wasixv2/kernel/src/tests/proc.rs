//! fork: clone and clone3 without `CLONE_VM`; the kernel's init.

use wasixv2_abi::{
    CloneArgs, CloneArgsSize, CloneFlags, ConstRlimit64Ptr, Idtype, Pid, PrctlOption, Rlimit64Ptr,
    RlimitResource, RusagePtr, S32Ptr, Sigaction, SiginfoPtr, Signal, Sigset, WaitOptions,
};
use wasixv2_syscalls::{read_value, write_value};

use futures::FutureExt as _;

use super::{
    ipc::{errno, put},
    *,
};
use crate::sync::MutexExt;

/// An address space the engine can duplicate, as the asyncify configuration's is.
pub(super) struct ForkableMemory(pub(super) FlatMemory);

impl UserMemory for ForkableMemory {
    fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        self.0.read(addr, buf)
    }

    fn write<'a>(&'a self, addr: u32, data: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        self.0.write(addr, data)
    }

    fn size(&self) -> LocalFuture<'_, u64> {
        self.0.size()
    }

    fn grow(&self, pages: u32) -> LocalFuture<'_, Option<u64>> {
        self.0.grow(pages)
    }

    fn can_duplicate(&self) -> LocalFuture<'_, bool> {
        Box::pin(async { true })
    }
}

pub(super) fn forkable() -> ForkableMemory {
    ForkableMemory(FlatMemory(RefCell::new(vec![0; 256])))
}

fn clone(kernel: &Kernel, caller: &Caller<'_>, flags: u32, newsp: u32) -> SysResult<i32> {
    let (parent_tid, child_tid) = (S32Ptr::new(8), S32Ptr::new(16));
    block_on(kernel.clone(caller, flags, newsp, parent_tid, 0, child_tid))
}

const SIGCHLD: u32 = Signal::SIGCHLD.0 as u32;

#[test]
fn fork_needs_an_address_space_that_can_be_duplicated() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 64]));
    let caller = Caller::new(task, &memory);

    let ret = clone(&kernel, &caller, SIGCHLD, 0);

    assert_eq!(ret, Err(SysErr::Errno(Errno::ENOSYS)));
    assert_eq!(kernel.take_pending(task), None);
    assert!(kernel.procs.find(task.0 + 1).is_none());
}

/// Where the memory cannot be copied, fork's `ENOSYS` comes only after the checks Linux makes
/// in `copy_process`, so a caller that may not fork at all hears why: no namespace without
/// `CAP_SYS_ADMIN` (`EPERM`). What a failed fork made is undone: the first process of a pid
/// namespace that a failed fork would have started is still pid 1 there.
#[test]
fn a_fork_that_cannot_copy_memory_reports_copy_processs_errors_first() {
    let (kernel, task, memory) = super::cred::machine_as(0, 0);
    let caller = Caller::new(task, &memory);
    let newpid = CloneFlags::CLONE_NEWPID.0 as u32;

    assert_eq!(
        block_on(kernel.unshare(&caller, newpid)),
        Ok(0),
        "children start in a new pid namespace"
    );
    assert_eq!(
        clone(&kernel, &caller, SIGCHLD, 0),
        Err(SysErr::Errno(Errno::ENOSYS))
    );
    assert_eq!(kernel.take_pending(task), None);
    let forkable = forkable();
    let forker = Caller::new(task, &forkable);
    assert!(clone(&kernel, &forker, SIGCHLD, 0).is_ok());
    let Some(GuestAction::Fork { child, .. }) = kernel.take_pending(task) else {
        panic!("fork left no child");
    };
    assert_eq!(kernel.own_tid(child), 1, "the namespace's init");

    let (kernel, task, memory) = super::cred::machine_as(1, 1);
    let caller = Caller::new(task, &memory);
    for flag in [CloneFlags::CLONE_NEWNS, CloneFlags::CLONE_NEWPID] {
        assert_eq!(
            clone(&kernel, &caller, SIGCHLD | flag.0 as u32, 0),
            Err(SysErr::Errno(Errno::EPERM)),
            "{flag:?}"
        );
    }
    assert_eq!(
        clone(&kernel, &caller, SIGCHLD, 0),
        Err(SysErr::Errno(Errno::ENOSYS))
    );
}

#[test]
fn fork_leaves_the_copy_and_tid_writes_to_the_execution_layer() {
    let (kernel, task, _) = setup(b"");
    let memory = forkable();
    let caller = Caller::new(task, &memory);
    let flags = SIGCHLD
        | CloneFlags::CLONE_PARENT_SETTID.0 as u32
        | CloneFlags::CLONE_CHILD_SETTID.0 as u32;

    let child = TaskId(clone(&kernel, &caller, flags, 0).expect("fork") as u32);

    assert_ne!(child, task);
    assert_eq!(
        word(&memory.0, 8),
        0,
        "the parent's tid write waits for the copy"
    );
    assert_eq!(
        kernel.take_pending(task),
        Some(GuestAction::Fork {
            child,
            pid: child.0 as i32,
            tid: child.0 as i32,
            parent_tid: Some(8),
            pidfd: None,
            child_tid: Some(16),
            tls: None,
            vfork: false,
        })
    );
}

#[test]
fn a_forked_child_is_reaped_by_its_parent() {
    let (kernel, task, _) = setup(b"");
    let memory = forkable();
    let caller = Caller::new(task, &memory);
    let child = TaskId(clone(&kernel, &caller, SIGCHLD, 0).expect("fork") as u32);
    kernel.take_pending(task);
    let child_memory = forkable();
    let child_caller = Caller::new(child, &child_memory);

    assert_eq!(block_on(kernel.getppid(&child_caller)), Ok(task.0 as Pid));
    assert_eq!(block_on(kernel.exit_group(&child_caller, 7)), Ok(()));
    let waited = block_on(kernel.waitid(
        &caller,
        Idtype::P_PID,
        child.0 as Pid,
        SiginfoPtr::new(32),
        WaitOptions::WEXITED,
        RusagePtr::new(0),
    ));

    assert_eq!(waited, Ok(0));
    assert_eq!(word(&memory.0, 32 + 12), child.0, "si_pid");
    assert_eq!(word(&memory.0, 32 + 20), 7, "si_status");
}

#[test]
fn fork_rejects_a_new_stack() {
    let (kernel, task, _) = setup(b"");
    let memory = forkable();
    let caller = Caller::new(task, &memory);

    let stack = clone(&kernel, &caller, SIGCHLD, 4096);

    assert_eq!(stack, Err(SysErr::Errno(Errno::EINVAL)));
}

/// fork with `CLONE_VFORK`: the parent's clone returns once the child is done with its copy
/// of the memory (`wait_for_vfork_done`), here as it exits (`exit_mm`'s `mm_release`).
#[test]
fn a_vfork_without_clone_vm_waits_until_the_child_exits() {
    let (kernel, task, _) = setup(b"");
    let memory = forkable();
    let caller = Caller::new(task, &memory);
    let flags = SIGCHLD | CloneFlags::CLONE_VFORK.0 as u32;

    let child = TaskId(clone(&kernel, &caller, flags, 0).expect("fork") as u32);

    let Some(GuestAction::Fork { vfork, .. }) = kernel.take_pending(task) else {
        panic!("fork left no child");
    };
    assert!(vfork);
    let mut wait = Box::pin(kernel.vfork_wait(task));
    assert!(wait.as_mut().now_or_never().is_none(), "the child runs");
    let child_memory = forkable();
    let child_caller = Caller::new(child, &child_memory);
    assert_eq!(block_on(kernel.exit_group(&child_caller, 3)), Ok(()));
    assert!(
        wait.as_mut().now_or_never().is_some(),
        "its exit lets the parent go"
    );
    // Waiting again finds nothing to wait for.
    assert!(kernel.vfork_wait(task).now_or_never().is_some());
}

/// A vfork child with memory of its own lets its parent go once its `execve` replaced that
/// memory (`exec_mm_release`), and runs on.
#[test]
fn a_vfork_without_clone_vm_waits_until_the_child_execs() {
    let (kernel, task, _) = setup(b"");
    let memory = forkable();
    let caller = Caller::new(task, &memory);
    let flags = SIGCHLD | CloneFlags::CLONE_VFORK.0 as u32;
    let child = TaskId(clone(&kernel, &caller, flags, 0).expect("fork") as u32);
    while kernel.take_pending(task).is_some() {}
    let mut wait = Box::pin(kernel.vfork_wait(task));
    let child_memory = FlatMemory(RefCell::new(vec![0; 65536]));
    let child_caller = Caller::new(child, &child_memory);
    super::task::install_prog(&kernel, &child_caller, &child_memory);
    assert!(wait.as_mut().now_or_never().is_none());

    assert_eq!(block_on(super::task::execve(&kernel, &child_caller)), Ok(0));

    assert!(wait.as_mut().now_or_never().is_some());
    let exec = GuestAction::Exec { vfork_parent: None };
    assert_eq!(kernel.take_pending(child), Some(exec));
}

/// A fork child that never runs (the execution layer could not copy the memory) lets its
/// vfork parent go as it is released.
#[test]
fn a_vfork_child_that_never_runs_lets_the_parent_go() {
    let (kernel, task, _) = setup(b"");
    let memory = forkable();
    let caller = Caller::new(task, &memory);
    let flags = SIGCHLD | CloneFlags::CLONE_VFORK.0 as u32;
    let child = TaskId(clone(&kernel, &caller, flags, 0).expect("fork") as u32);
    while kernel.take_pending(task).is_some() {}
    let mut wait = Box::pin(kernel.vfork_wait(task));
    assert!(wait.as_mut().now_or_never().is_none());

    assert!(kernel.fault(child, Signal::SIGSEGV.0));
    kernel.release(child);

    assert!(wait.as_mut().now_or_never().is_some());
}

/// `wait_for_vfork_done` is killable: SIGKILL ends the parent's wait and its thread group,
/// while the child goes on.
#[test]
fn a_vfork_parent_dies_of_sigkill_while_it_waits() {
    let (kernel, task, _) = setup(b"");
    let memory = forkable();
    let caller = Caller::new(task, &memory);
    let flags = SIGCHLD | CloneFlags::CLONE_VFORK.0 as u32;
    let child = TaskId(clone(&kernel, &caller, flags, 0).expect("fork") as u32);
    while kernel.take_pending(task).is_some() {}
    let mut wait = Box::pin(kernel.vfork_wait(task));
    assert!(wait.as_mut().now_or_never().is_none());

    let child_memory = forkable();
    let child_caller = Caller::new(child, &child_memory);
    let pid = task.0 as Pid;
    assert_eq!(
        block_on(kernel.sys_kill(&child_caller, pid, Signal::SIGKILL)),
        Ok(0)
    );

    assert!(wait.as_mut().now_or_never().is_some());
    assert_eq!(
        kernel.take_pending(task),
        Some(GuestAction::Exit(128 + Signal::SIGKILL.0))
    );
    assert_eq!(kernel.take_pending(child), None, "the child goes on");
    assert_eq!(block_on(kernel.exit_group(&child_caller, 0)), Ok(()));
}

#[test]
fn fork_fails_with_eagain_at_rlimit_nproc() {
    let (kernel, task, _) = setup(b"");
    let memory = forkable();
    let caller = Caller::new(task, &memory);
    // The caller's own task already counts: a limit of 1 leaves no room for a child.
    let limit = [1u64.to_le_bytes(), 1u64.to_le_bytes()].concat();
    block_on(memory.write(64, &limit)).unwrap();
    let set = block_on(kernel.prlimit64(
        &caller,
        0,
        RlimitResource::RLIMIT_NPROC,
        ConstRlimit64Ptr::new(64),
        Rlimit64Ptr::new(0),
    ));
    assert_eq!(set, Ok(0));

    assert_eq!(
        clone(&kernel, &caller, SIGCHLD, 0),
        Err(SysErr::Errno(Errno::EAGAIN))
    );
    assert_eq!(kernel.take_pending(task), None);
}

/// A process killed before its image starts, as when the embedder kills it, does not exec
/// (`de_thread`'s EAGAIN), and the exit stays queued to end it.
#[test]
fn kernel_execve_of_a_killed_process_fails_and_keeps_the_exit() {
    const IMAGE: &[u8] = b"\0asm\x01\0\0\0";
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; WASM_PAGE_SIZE as usize]));
    let caller = Caller::new(task, &memory);
    put(&memory, 32, b"/prog\0");
    put(&memory, 16, IMAGE);
    let create = OpenFlags::O_CREAT | OpenFlags::O_WRONLY;
    let fd = block_on(kernel.openat(&caller, Fd(-100), GuestPtr::new(32), create, 0o755)).unwrap();
    let written = block_on(kernel.write(&caller, fd, UserSlice::new(16, IMAGE.len() as u32)));
    assert_eq!(written, Ok(IMAGE.len() as u32));
    assert_eq!(block_on(kernel.close(&caller, fd)), Ok(0));

    assert!(kernel.force_fatal_signal(task, Signal::SIGKILL.0));
    let image = block_on(kernel.exec(task, b"/prog", vec![b"prog".to_vec()], Vec::new()));

    assert_eq!(image.err(), Some(Errno::EAGAIN));
    assert!(kernel.process(task).exe.lock_or_recover().is_none());
    assert_eq!(
        kernel.take_pending(task),
        Some(GuestAction::Exit(128 + Signal::SIGKILL.0))
    );
}

/// An exited process's descriptors close by its pid alone. Two of its tasks released at once
/// can leave one collecting the descriptor tables after the other took the leader's task away.
#[test]
fn exit_closes_the_files_after_the_leader_is_gone() {
    const THREAD: u32 = (CloneFlags::CLONE_VM.0
        | CloneFlags::CLONE_FS.0
        | CloneFlags::CLONE_FILES.0
        | CloneFlags::CLONE_SIGHAND.0
        | CloneFlags::CLONE_THREAD.0) as u32;
    let (kernel, leader, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; WASM_PAGE_SIZE as usize]));
    let caller = Caller::new(leader, &memory);
    put(&memory, 32, b"/f\0");
    let create = OpenFlags::O_CREAT | OpenFlags::O_RDWR;
    block_on(kernel.openat(&caller, Fd(-100), GuestPtr::new(32), create, 0o644)).unwrap();
    let null = GuestPtrMut::new(0);
    block_on(kernel.thread_spawn(&caller, THREAD, 0, null, 0, null)).unwrap();
    let process = kernel.process(leader);

    thread::forget_task(&mut kernel.state.lock_or_recover(), leader);
    kernel.exit_files(&process);
}

/// Records what `execve` shows a format handler, and refuses what it is told to.
struct Recording {
    refuse_header: bool,
    seen: Mutex<Vec<(&'static str, usize)>>,
}

impl wasixv2_syscalls::Binfmt for Recording {
    fn probe(&self, header: &[u8]) -> Result<(), Errno> {
        self.seen.lock_or_recover().push(("probe", header.len()));
        match self.refuse_header {
            true => Err(Errno::ENOEXEC),
            false => Ok(()),
        }
    }

    fn check(&self, image: &wasixv2_syscalls::Image) -> Result<(), Errno> {
        self.seen
            .lock_or_recover()
            .push(("check", image.bytes().len()));
        Err(Errno::ENOEXEC)
    }
}

/// `search_binary_handler`: a handler first sees the file's first `BINPRM_BUF_SIZE` bytes, and
/// the whole file is read only for one that takes them; either may refuse it before the point
/// of no return.
#[test]
fn execve_probes_the_header_before_it_reads_the_image() {
    let image = [b"\0asm\x01\0\0\0".as_slice(), &[7; 5000]].concat();
    for refuse_header in [true, false] {
        let (mut kernel, task, _) = setup(b"");
        let binfmt = Arc::new(Recording {
            refuse_header,
            seen: Mutex::default(),
        });
        kernel.set_binfmt(binfmt.clone());
        let memory = FlatMemory(RefCell::new(vec![0; 2 * WASM_PAGE_SIZE as usize]));
        let caller = Caller::new(task, &memory);
        put(&memory, 32, b"/prog\0");
        put(&memory, 4096, &image);
        let create = OpenFlags::O_CREAT | OpenFlags::O_WRONLY;
        let fd =
            block_on(kernel.openat(&caller, Fd(-100), GuestPtr::new(32), create, 0o755)).unwrap();
        let slice = UserSlice::new(4096, image.len() as u32);
        assert_eq!(
            block_on(kernel.write(&caller, fd, slice)),
            Ok(image.len() as u32)
        );
        assert_eq!(block_on(kernel.close(&caller, fd)), Ok(0));

        let ran = block_on(kernel.exec(task, b"/prog", vec![b"prog".to_vec()], Vec::new()));

        assert_eq!(ran.err(), Some(Errno::ENOEXEC));
        let seen = binfmt.seen.lock_or_recover().clone();
        match refuse_header {
            true => assert_eq!(seen, [("probe", BINPRM_BUF_SIZE)]),
            false => assert_eq!(seen, [("probe", BINPRM_BUF_SIZE), ("check", image.len())]),
        }
    }
}

// ---- process groups ----

/// `getpgid`, `getsid` and `setpgid` look a negative pid up as any other that no process has
/// (`find_task_by_vpid`): `ESRCH`. A negative group is `EINVAL`, and a group that does not
/// exist in the caller's session `EPERM`.
#[test]
fn a_negative_pid_names_no_process() {
    let (kernel, task, _) = setup(b"");
    let memory = forkable();
    let caller = Caller::new(task, &memory);
    let esrch = Err(SysErr::Errno(Errno::ESRCH));

    assert_eq!(block_on(kernel.getpgid(&caller, -99)), esrch);
    assert_eq!(block_on(kernel.getsid(&caller, -99)), esrch);
    assert_eq!(block_on(kernel.setpgid(&caller, -1, 0)), esrch);
    assert_eq!(block_on(kernel.setpgid(&caller, 99, 0)), esrch);
    assert_eq!(
        block_on(kernel.setpgid(&caller, 0, -1)),
        Err(SysErr::Errno(Errno::EINVAL))
    );
    assert_eq!(
        block_on(kernel.setpgid(&caller, 0, 4_194_304)),
        Err(SysErr::Errno(Errno::EPERM))
    );
}

// ---- reparenting ----

pub(super) fn fork_child(kernel: &Kernel, caller: &Caller<'_>) -> TaskId {
    let child = TaskId(clone(kernel, caller, SIGCHLD, 0).expect("fork") as u32);
    while kernel.take_pending(caller.task()).is_some() {}
    child
}

/// A zombie handed to a subreaper that waits for its children is reported to it with SIGCHLD
/// (`reparent_leader`) and stays until reaped.
#[test]
fn a_zombie_handed_to_a_subreaper_is_reported_and_stays() {
    let (kernel, task, _) = setup(b"");
    let memory = forkable();
    let caller = Caller::new(task, &memory);
    let subreaper = kernel.prctl(&caller, PrctlOption::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0);
    assert_eq!(block_on(subreaper), Ok(0));
    // A SIGCHLD with the default action is dropped unless blocked.
    let sigchld = 1 << (Signal::SIGCHLD.0 - 1);
    kernel.task_signals(task).replace_blocked(sigchld);
    let middle = fork_child(&kernel, &caller);
    let middle_memory = forkable();
    let middle_caller = Caller::new(middle, &middle_memory);
    let zombie = fork_child(&kernel, &middle_caller);
    let zombie_memory = forkable();
    assert_eq!(
        block_on(kernel.exit_group(&Caller::new(zombie, &zombie_memory), 4)),
        Ok(())
    );
    kernel.release(zombie);

    assert_eq!(block_on(kernel.exit_group(&middle_caller, 0)), Ok(()));

    // The zombie's report comes first; the middle process's own SIGCHLD finds it pending.
    let signals = kernel.task_signals(task);
    let info = signals.dequeue_from(sigchld).expect("SIGCHLD");
    let si_pid = i32::from_le_bytes(info.sifields[0..4].try_into().unwrap());
    assert_eq!(si_pid, zombie.0 as Pid);
    let waited = block_on(kernel.waitid(
        &caller,
        Idtype::P_PID,
        zombie.0 as Pid,
        SiginfoPtr::new(32),
        WaitOptions::WEXITED,
        RusagePtr::new(0),
    ));
    assert_eq!(waited, Ok(0));
    assert_eq!(word(&memory.0, 32 + 20), 4, "si_status");
}

// ---- init ----

/// A kernel booted with its init, and a process it spawned.
fn setup_with_init() -> (Kernel, TaskId) {
    let stdin = Arc::new(Input(Mutex::new(Vec::new())));
    let (kernel, task, _) = spawn_on(test_kernel().with_init(), stdin, &[], &[]);
    (kernel, task)
}

const INIT: TaskId = TaskId(1);

#[test]
fn spawned_processes_are_children_of_init_when_the_kernel_has_one() {
    let (kernel, task) = setup_with_init();
    let memory = forkable();
    let caller = Caller::new(task, &memory);
    assert_eq!(task, TaskId(2));
    assert_eq!(block_on(kernel.getppid(&caller)), Ok(1));
    assert_eq!(block_on(kernel.getpid(&caller)), Ok(2));

    let (kernel, task, _) = setup(b"");
    let caller = Caller::new(task, &memory);
    assert_eq!(task, TaskId(1));
    assert_eq!(block_on(kernel.getppid(&caller)), Ok(0));
}

/// `forget_original_parent`: the children of a process that exits go to init, which reaps
/// those that are zombies already at once, and the others as they exit.
#[test]
fn orphans_go_to_init_which_reaps_them() {
    let (kernel, task) = setup_with_init();
    let memory = forkable();
    let caller = Caller::new(task, &memory);
    let running = fork_child(&kernel, &caller);
    let zombie = fork_child(&kernel, &caller);
    let (running_memory, zombie_memory) = (forkable(), forkable());
    let running_caller = Caller::new(running, &running_memory);
    let zombie_caller = Caller::new(zombie, &zombie_memory);
    assert_eq!(block_on(kernel.exit_group(&zombie_caller, 3)), Ok(()));
    kernel.release(zombie);
    assert!(
        kernel.procs.find(zombie.0).is_some(),
        "a zombie of its parent"
    );

    assert_eq!(block_on(kernel.exit_group(&caller, 0)), Ok(()));

    assert!(
        kernel.procs.find(task.0).is_none(),
        "init reaped the parent"
    );
    assert!(
        kernel.procs.find(zombie.0).is_none(),
        "init reaped the zombie"
    );
    assert_eq!(block_on(kernel.getppid(&running_caller)), Ok(1));
    assert_eq!(block_on(kernel.exit_group(&running_caller, 0)), Ok(()));
    assert!(
        kernel.procs.find(running.0).is_none(),
        "init reaped the orphan"
    );
    assert!(kernel.procs.find(INIT.0).is_some());
}

/// `SIGNAL_UNKILLABLE`: a signal init has no handler for is dropped as it is sent, SIGKILL
/// included, and `kill(-1)` passes init by.
#[test]
fn init_takes_no_signal_it_has_no_handler_for() {
    let (kernel, task) = setup_with_init();
    let memory = forkable();
    let caller = Caller::new(task, &memory);
    let child = fork_child(&kernel, &caller);

    for sig in [Signal::SIGKILL, Signal::SIGTERM, Signal::SIGSTOP] {
        assert_eq!(block_on(kernel.sys_kill(&caller, 1, sig)), Ok(0));
    }
    assert_eq!(Kernel::kill(&kernel, INIT, Signal::SIGKILL), Ok(()));
    assert_eq!(
        block_on(kernel.sys_kill(&caller, -1, Signal::SIGTERM)),
        Ok(0)
    );

    assert_eq!(kernel.take_pending(INIT), None);
    assert!(!kernel.process(INIT).group.exiting());
    assert_eq!(
        kernel.take_pending(child),
        Some(GuestAction::Exit(128 + 15))
    );
    assert_eq!(
        kernel.take_pending(task),
        None,
        "kill(-1) spares the caller"
    );
}

/// `terminate` ends every process but init, which has no instance to release.
#[test]
fn terminate_returns_while_init_lives_on() {
    use futures::FutureExt as _;

    let (kernel, task) = setup_with_init();
    let mut terminate = std::pin::pin!(kernel.terminate());
    assert!(terminate.as_mut().now_or_never().is_none());
    assert!(matches!(
        kernel.take_pending(task),
        Some(GuestAction::Exit(_))
    ));
    kernel.release(task);
    assert_eq!(terminate.now_or_never(), Some(()));
    assert!(kernel.procs.find(INIT.0).is_some());
}

/// `clone3` with `args` at 0x8000, passing `size`.
fn clone3(kernel: &Kernel, caller: &Caller<'_>, args: &CloneArgs, size: u32) -> SysResult<i32> {
    block_on(write_value(caller.memory(), GuestPtrMut::new(0x8000), args)).unwrap();
    block_on(kernel.clone3(caller, GuestPtr::new(0x8000), size))
}

fn big_forkable() -> ForkableMemory {
    ForkableMemory(FlatMemory(RefCell::new(vec![0; 65536])))
}

const ARGS_SIZE: u32 = CloneArgsSize::CLONE_ARGS_SIZE_VER2.0;

#[test]
fn clone3_forks_as_clone_does() {
    let (kernel, task, _) = setup(b"");
    let memory = big_forkable();
    let caller = Caller::new(task, &memory);
    let args = CloneArgs {
        flags: CloneFlags::CLONE_PARENT_SETTID,
        parent_tid: 8,
        exit_signal: u64::from(SIGCHLD),
        ..CloneArgs::default()
    };

    let child = TaskId(clone3(&kernel, &caller, &args, ARGS_SIZE).expect("fork") as u32);

    assert_eq!(
        kernel.take_pending(task),
        Some(GuestAction::Fork {
            child,
            pid: child.0 as i32,
            tid: child.0 as i32,
            parent_tid: Some(8),
            pidfd: None,
            child_tid: None,
            tls: None,
            vfork: false,
        })
    );
    let node = kernel.procs.tree.lock().unwrap().nodes[&child.0].exit_signal;
    assert_eq!(node, SIGCHLD as i32);
}

#[test]
fn clone3_takes_any_size_of_its_arguments_with_zeros_past_what_it_knows() {
    let (kernel, task, _) = setup(b"");
    let memory = big_forkable();
    let caller = Caller::new(task, &memory);
    let args = CloneArgs {
        exit_signal: u64::from(SIGCHLD),
        ..CloneArgs::default()
    };
    let size = |size| clone3(&kernel, &caller, &args, size);

    assert_eq!(errno(size(63)), Errno::EINVAL, "below the first version");
    assert_eq!(errno(size(70_000)), Errno::E2BIG, "beyond a page");
    assert!(size(64).is_ok(), "the first version");
    put(&memory.0, 0x8000 + ARGS_SIZE, &[0; 8]);
    assert!(size(ARGS_SIZE + 8).is_ok(), "zeros past the known version");
    put(&memory.0, 0x8000 + ARGS_SIZE + 4, &[1]);
    assert_eq!(errno(size(ARGS_SIZE + 8)), Errno::E2BIG);
    let end = memory.0.0.borrow().len() as u32;
    let at_end = block_on(kernel.clone3(&caller, GuestPtr::new(end - 64), 128));
    assert_eq!(errno(at_end), Errno::EFAULT);
}

#[test]
fn clone3_rejects_what_clone3_args_valid_does() {
    let (kernel, task, _) = setup(b"");
    let memory = big_forkable();
    let caller = Caller::new(task, &memory);
    let fork = CloneArgs {
        exit_signal: u64::from(SIGCHLD),
        ..CloneArgs::default()
    };
    let cases = [
        (
            "signal past CSIGNAL",
            CloneArgs {
                exit_signal: 0x100,
                ..fork
            },
        ),
        (
            "signal past _NSIG",
            CloneArgs {
                exit_signal: 65,
                ..fork
            },
        ),
        (
            "signal byte in flags",
            CloneArgs {
                flags: CloneFlags(0x11),
                ..fork
            },
        ),
        (
            "CLONE_DETACHED",
            CloneArgs {
                flags: CloneFlags::CLONE_DETACHED,
                ..fork
            },
        ),
        (
            "a stack without a size",
            CloneArgs {
                stack: 0x1000,
                ..fork
            },
        ),
        (
            "a size without a stack",
            CloneArgs {
                stack_size: 4,
                ..fork
            },
        ),
        (
            "an unknown flag",
            CloneArgs {
                flags: CloneFlags(1 << 40),
                ..fork
            },
        ),
        (
            "set_tid without a size",
            CloneArgs {
                set_tid: 0x100,
                ..fork
            },
        ),
        (
            "CLONE_SIGHAND with CLONE_CLEAR_SIGHAND",
            CloneArgs {
                flags: CloneFlags::CLONE_SIGHAND
                    | CloneFlags::CLONE_VM
                    | CloneFlags::CLONE_CLEAR_SIGHAND,
                ..fork
            },
        ),
        (
            "an exit signal with CLONE_PARENT",
            CloneArgs {
                flags: CloneFlags::CLONE_PARENT,
                ..fork
            },
        ),
        (
            "CLONE_NEWNS with CLONE_FS",
            CloneArgs {
                flags: CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_FS,
                ..fork
            },
        ),
        (
            "CLONE_NEWUSER with CLONE_FS",
            CloneArgs {
                flags: CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_FS,
                ..fork
            },
        ),
    ];
    for (what, args) in cases {
        assert_eq!(
            errno(clone3(&kernel, &caller, &args, ARGS_SIZE)),
            Errno::EINVAL,
            "{what}"
        );
    }
    assert_eq!(kernel.take_pending(task), None);
}

/// `copy_clone_args_from_user` reads `set_tid` before `clone3_args_valid` looks at anything
/// else, so a fault there comes before invalid flags, a stack without a size or a bad exit
/// signal; and its size is checked before it is read.
#[test]
fn clone3_reads_set_tid_before_it_checks_the_arguments() {
    let (kernel, task, _) = setup(b"");
    let memory = big_forkable();
    let caller = Caller::new(task, &memory);
    let end = memory.0.0.borrow().len() as u64;
    let bad_set_tid = CloneArgs {
        set_tid: end - 2,
        set_tid_size: 1,
        ..CloneArgs::default()
    };
    let cases = [
        (
            "unknown flags",
            CloneArgs {
                flags: CloneFlags(1 << 40),
                ..bad_set_tid
            },
        ),
        (
            "a reserved flag",
            CloneArgs {
                flags: CloneFlags::CLONE_DETACHED,
                ..bad_set_tid
            },
        ),
        (
            "a stack without a size",
            CloneArgs {
                stack: 0x4000,
                ..bad_set_tid
            },
        ),
        (
            "CLONE_THREAD with an exit signal",
            CloneArgs {
                flags: CloneFlags::CLONE_THREAD,
                exit_signal: u64::from(SIGCHLD),
                ..bad_set_tid
            },
        ),
    ];
    for (what, args) in cases {
        assert_eq!(
            errno(clone3(&kernel, &caller, &args, ARGS_SIZE)),
            Errno::EFAULT,
            "{what}"
        );
    }
    let too_many = CloneArgs {
        set_tid_size: 33,
        ..bad_set_tid
    };
    assert_eq!(
        errno(clone3(&kernel, &caller, &too_many, ARGS_SIZE)),
        Errno::EINVAL,
        "more pids than pid namespace levels"
    );
    assert_eq!(kernel.take_pending(task), None);
}

#[test]
fn clone3_new_namespaces_chosen_pids_and_cgroups_are_refused() {
    let (kernel, task, _) = setup(b"");
    let memory = big_forkable();
    let caller = Caller::new(task, &memory);
    let fork = CloneArgs {
        exit_signal: u64::from(SIGCHLD),
        ..CloneArgs::default()
    };
    put(&memory.0, 0x100, &1i32.to_le_bytes());

    let uts = CloneArgs {
        flags: CloneFlags::CLONE_NEWUTS,
        ..fork
    };
    let set_tid = CloneArgs {
        set_tid: 0x100,
        set_tid_size: 1,
        ..fork
    };
    let cgroup = CloneArgs {
        flags: CloneFlags::CLONE_INTO_CGROUP,
        cgroup: 0,
        ..fork
    };

    assert_eq!(
        errno(clone3(&kernel, &caller, &uts, ARGS_SIZE)),
        Errno::EPERM
    );
    assert_eq!(
        errno(clone3(&kernel, &caller, &set_tid, ARGS_SIZE)),
        Errno::EPERM,
        "a chosen pid needs CAP_CHECKPOINT_RESTORE"
    );
    assert_eq!(
        errno(clone3(&kernel, &caller, &cgroup, ARGS_SIZE)),
        Errno::EBADF,
        "no descriptor is a cgroup"
    );
    assert_eq!(kernel.take_pending(task), None);
}

#[test]
fn clone3_clear_sighand_gives_the_child_default_handlers() {
    let (kernel, task, _) = setup(b"");
    let memory = big_forkable();
    let caller = Caller::new(task, &memory);
    let action = |handler: u32| Sigaction {
        sa_handler: handler,
        sa_flags: 0,
        sa_mask: Sigset { sig: [0; 2] },
    };
    assert_eq!(block_on(kernel.signal_callback_register(&caller, 3)), Ok(0));
    for (sig, handler) in [(Signal::SIGUSR1, 7), (Signal::SIGUSR2, 1)] {
        block_on(write_value(
            &memory,
            GuestPtrMut::new(0x140),
            &action(handler),
        ))
        .unwrap();
        let set = kernel.rt_sigaction(&caller, sig, GuestPtr::new(0x140), GuestPtrMut::NULL, 8);
        assert_eq!(block_on(set), Ok(0));
    }
    let args = CloneArgs {
        flags: CloneFlags::CLONE_CLEAR_SIGHAND,
        exit_signal: u64::from(SIGCHLD),
        ..CloneArgs::default()
    };

    let child = TaskId(clone3(&kernel, &caller, &args, ARGS_SIZE).unwrap() as u32);

    let child_memory = big_forkable();
    let child_caller = Caller::new(child, &child_memory);
    let handler = |caller: &Caller<'_>, sig: Signal| {
        let old = GuestPtrMut::new(0x200);
        let get = kernel.rt_sigaction(caller, sig, GuestPtr::NULL, old, 8);
        assert_eq!(block_on(get), Ok(0));
        block_on(read_value::<Sigaction>(
            caller.memory(),
            GuestPtr::new(0x200),
        ))
        .unwrap()
        .sa_handler
    };
    assert_eq!(
        handler(&child_caller, Signal::SIGUSR1),
        0,
        "caught: default"
    );
    assert_eq!(
        handler(&child_caller, Signal::SIGUSR2),
        1,
        "ignored: still ignored"
    );
    assert_eq!(handler(&caller, Signal::SIGUSR1), 7, "the parent's stay");
}

/// The pidfd `fd` of `task`: whose process it refers to, and whether it is close-on-exec.
fn pidfd_of(kernel: &Kernel, task: TaskId, fd: Fd) -> Option<(u32, bool)> {
    let file = kernel.file(task, fd).ok()?;
    let cloexec = kernel.files(task).lock().unwrap().cloexec(fd)?;
    Some((crate::process::pidfd_target(&file)?, cloexec))
}

/// `clone3` with `CLONE_PIDFD` gives the parent a close-on-exec pidfd of the child at its
/// lowest free descriptor; its number reaches `pidfd` only in the parent's address space, so
/// the write waits for the copy, and the child's descriptor table has none (`copy_process`
/// prepares it after `copy_files`).
#[test]
fn clone3_pidfd_gives_the_parent_a_pidfd_of_the_child() {
    let (kernel, task, _) = setup(b"");
    let memory = big_forkable();
    let caller = Caller::new(task, &memory);
    let args = CloneArgs {
        flags: CloneFlags::CLONE_PIDFD,
        pidfd: 0x100,
        exit_signal: u64::from(SIGCHLD),
        ..CloneArgs::default()
    };

    let child = TaskId(clone3(&kernel, &caller, &args, ARGS_SIZE).expect("fork") as u32);

    assert_eq!(pidfd_of(&kernel, task, Fd(3)), Some((child.0, true)));
    assert_eq!(word(&memory.0, 0x100), 0, "the write waits for the copy");
    assert_eq!(
        kernel.take_pending(task),
        Some(GuestAction::Fork {
            child,
            pid: child.0 as i32,
            tid: child.0 as i32,
            parent_tid: None,
            pidfd: Some((0x100, 3)),
            child_tid: None,
            tls: None,
            vfork: false,
        })
    );
    assert!(kernel.file(child, Fd(3)).is_err());
}

/// A pidfd that cannot be written faults before anything is made (`copy_process`'s `put_user`
/// fails into `bad_fork_put_pidfd`): `EFAULT` past the end of memory, in the low guard's
/// place, and on a page the address space maps `PROT_NONE`; no child, no descriptor.
#[test]
fn clone3_pidfd_that_cannot_be_written_makes_nothing() {
    let (kernel, task, _) = setup(b"");
    let memory = big_forkable();
    let caller = Caller::new(task, &memory);
    let anon = wasixv2_abi::MapFlags::MAP_PRIVATE | wasixv2_abi::MapFlags::MAP_ANONYMOUS;
    let prot_none = wasixv2_abi::ProtFlags(0);
    let page = block_on(kernel.mmap(&caller, 0, 4096, prot_none, anon, Fd(-1), 0)).unwrap();
    let end = block_on(memory.size()) as u32;

    for at in [end - 2, page] {
        let args = CloneArgs {
            flags: CloneFlags::CLONE_PIDFD,
            pidfd: u64::from(at),
            exit_signal: u64::from(SIGCHLD),
            ..CloneArgs::default()
        };
        assert_eq!(
            errno(clone3(&kernel, &caller, &args, ARGS_SIZE)),
            Errno::EFAULT,
            "{at:#x}"
        );
    }

    assert_eq!(kernel.take_pending(task), None);
    assert!(
        kernel.file(task, Fd(3)).is_err(),
        "the reservation is given back"
    );
    assert!(kernel.procs.find(task.0 + 1).is_none());
}

/// A vfork with `CLONE_PIDFD` (through `clone`, whose `parent_tid` doubles as where the pidfd
/// goes) writes the number at once: the child runs on the parent's memory.
#[test]
fn vfork_pidfd_is_written_at_once() {
    let (kernel, task, _) = setup(b"");
    let memory = forkable();
    let caller = Caller::new(task, &memory);
    let flags = CloneFlags::CLONE_VM | CloneFlags::CLONE_VFORK | CloneFlags::CLONE_PIDFD;
    let flags = flags.0 as u32 | SIGCHLD;
    put(&memory.0, 8, &(-1i32).to_le_bytes());

    assert_eq!(clone(&kernel, &caller, flags, 0), Ok(0));

    assert_eq!(word(&memory.0, 8), 3);
    let child = task.0 + 1;
    assert_eq!(pidfd_of(&kernel, task, Fd(3)), Some((child, true)));
}

/// `CLONE_PIDFD` with a pidfd that faults leaves `EFAULT` behind the checks of the flags, and
/// `CLONE_AUTOREAP` is still refused.
#[test]
fn clone_pidfd_faults_after_the_flag_checks() {
    let (kernel, task, _) = setup(b"");
    let memory = big_forkable();
    let caller = Caller::new(task, &memory);
    let bad = S32Ptr::new(1 << 20);
    let pidfd = CloneFlags::CLONE_PIDFD.0 as u32 | SIGCHLD;
    let none = S32Ptr::new(0);
    let clone_at = |flags: u32| block_on(kernel.clone(&caller, flags, 0, bad, 0, none));

    assert_eq!(errno(clone_at(pidfd)), Errno::EFAULT);
    let detached = pidfd | CloneFlags::CLONE_DETACHED.0 as u32;
    assert_eq!(errno(clone_at(detached)), Errno::EINVAL);
    let autoreap = CloneArgs {
        flags: CloneFlags::CLONE_AUTOREAP,
        ..CloneArgs::default()
    };
    assert_eq!(
        errno(clone3(&kernel, &caller, &autoreap, ARGS_SIZE)),
        Errno::EINVAL
    );
    assert_eq!(kernel.take_pending(task), None);
}

// ---- processes on the caller's memory ----

/// `thread-spawn` with `flags`, and its tid writes at 8 and 16.
fn spawn(kernel: &Kernel, caller: &Caller<'_>, flags: u32) -> SysResult<i32> {
    let (parent_tid, child_tid) = (S32Ptr::new(8), S32Ptr::new(16));
    block_on(kernel.thread_spawn(caller, flags, 0x40, parent_tid, 0, child_tid))
}

const VM: u32 = CloneFlags::CLONE_VM.0 as u32;

/// `thread-spawn` with `CLONE_VM` but not `CLONE_THREAD` makes a process of its own on the
/// caller's memory: its own pid and thread group, the caller's process as its parent, which
/// reaps it with its exit signal, the one address space, and a thread-style start.
#[test]
fn clone_vm_without_clone_thread_makes_a_process_on_the_callers_memory() {
    let (kernel, task, _) = setup(b"");
    let memory = forkable();
    let caller = Caller::new(task, &memory);
    let flags = VM | SIGCHLD | CloneFlags::CLONE_PARENT_SETTID.0 as u32;

    let pid = spawn(&kernel, &caller, flags).expect("clone");

    let child = TaskId(pid as u32);
    assert_eq!(word(&memory.0, 8), child.0, "CLONE_PARENT_SETTID at once");
    assert_eq!(
        kernel.take_pending(task),
        Some(GuestAction::StartThread {
            task: child,
            tid: pid,
            start_arg: 0x40,
        })
    );
    assert_eq!(kernel.tgid(child), child, "a thread group of its own");
    assert!(Arc::ptr_eq(&kernel.mm(child), &kernel.mm(task)));
    let child_caller = Caller::new(child, &memory);
    assert_eq!(block_on(kernel.getppid(&child_caller)), Ok(task.0 as Pid));
    assert_eq!(block_on(kernel.exit(&child_caller, 6)), Ok(()));
    block_on(kernel.exit_mm(&child_caller));
    kernel.release(child);
    let waited = block_on(kernel.waitid(
        &caller,
        Idtype::P_PID,
        pid,
        SiginfoPtr::new(32),
        WaitOptions::WEXITED,
        RusagePtr::new(0),
    ));
    assert_eq!(waited, Ok(0));
    assert_eq!(word(&memory.0, 32), Signal::SIGCHLD.0 as u32, "si_signo");
    assert_eq!(word(&memory.0, 32 + 20), 6, "si_status");
}

/// The child's `clear_child_tid` is written on its exit, as another task (its creator) still
/// uses the memory (`mm_users > 1`), and the memory stays.
#[test]
fn a_clone_vm_child_clears_its_child_tid_as_it_exits() {
    let (kernel, task, _) = setup(b"");
    let memory = forkable();
    let caller = Caller::new(task, &memory);
    let flags = VM
        | SIGCHLD
        | CloneFlags::CLONE_CHILD_SETTID.0 as u32
        | CloneFlags::CLONE_CHILD_CLEARTID.0 as u32;
    let child = TaskId(spawn(&kernel, &caller, flags).unwrap() as u32);
    while kernel.take_pending(task).is_some() {}
    assert_eq!(word(&memory.0, 16), child.0);

    let child_caller = Caller::new(child, &memory);
    assert_eq!(block_on(kernel.exit(&child_caller, 0)), Ok(()));
    block_on(kernel.exit_mm(&child_caller));

    assert_eq!(word(&memory.0, 16), 0);
}

/// `CLONE_PARENT` makes the child its creator's sibling, and the forms `clone` refuses, a
/// vfork, and `CLONE_SIGHAND` without `CLONE_VM` stay `EINVAL`; `clone` itself cannot start a
/// process on the caller's memory.
#[test]
fn clone_vm_takes_clone_parent_and_refuses_what_clone_refuses() {
    let (kernel, task) = setup_with_init();
    let memory = forkable();
    let caller = Caller::new(task, &memory);

    let sibling = spawn(&kernel, &caller, VM | CloneFlags::CLONE_PARENT.0 as u32).unwrap();
    let sibling = Caller::new(TaskId(sibling as u32), &memory);
    assert_eq!(block_on(kernel.getppid(&sibling)), Ok(INIT.0 as Pid));
    while kernel.take_pending(task).is_some() {}

    let vfork = VM | CloneFlags::CLONE_VFORK.0 as u32 | SIGCHLD;
    let sighand = CloneFlags::CLONE_SIGHAND.0 as u32 | SIGCHLD;
    let fs_newns = VM | (CloneFlags::CLONE_FS | CloneFlags::CLONE_NEWNS).0 as u32;
    for flags in [vfork, sighand, SIGCHLD, fs_newns] {
        assert_eq!(
            errno(spawn(&kernel, &caller, flags)),
            Errno::EINVAL,
            "{flags:#x}"
        );
    }
    assert_eq!(
        errno(clone(&kernel, &caller, VM | SIGCHLD, 0)),
        Errno::EINVAL
    );
    assert_eq!(kernel.take_pending(task), None);
}

/// A process the execution layer could not start goes again, from the tree too, as
/// `copy_process` undoes a child.
#[test]
fn a_clone_vm_child_that_never_starts_leaves_no_process() {
    let (kernel, task, _) = setup(b"");
    let memory = forkable();
    let caller = Caller::new(task, &memory);
    let child = TaskId(spawn(&kernel, &caller, VM | SIGCHLD).unwrap() as u32);
    while kernel.take_pending(task).is_some() {}

    kernel.abort_thread(child);

    assert!(kernel.procs.find(child.0).is_none());
    let waited = block_on(kernel.waitid(
        &caller,
        Idtype::P_ALL,
        0,
        SiginfoPtr::new(32),
        WaitOptions(WaitOptions::WEXITED.0 | WaitOptions::WNOHANG.0),
        RusagePtr::new(0),
    ));
    assert_eq!(waited, Err(Errno::ECHILD.into()));
}

/// The handler of `sig` in the caller's process, as `rt_sigaction` reports it.
fn handler(kernel: &Kernel, caller: &Caller<'_>, sig: Signal) -> u32 {
    let old = GuestPtrMut::new(0x200);
    assert_eq!(
        block_on(kernel.rt_sigaction(caller, sig, GuestPtr::NULL, old, 8)),
        Ok(0)
    );
    block_on(read_value::<Sigaction>(
        caller.memory(),
        GuestPtr::new(0x200),
    ))
    .unwrap()
    .sa_handler
}

/// Sets the handler of `sig` in the caller's process.
fn set_handler(kernel: &Kernel, caller: &Caller<'_>, sig: Signal, handler: u32) {
    let act = Sigaction {
        sa_handler: handler,
        sa_flags: 0,
        sa_mask: Sigset { sig: [0; 2] },
    };
    block_on(write_value(caller.memory(), GuestPtrMut::new(0x140), &act)).unwrap();
    let set = kernel.rt_sigaction(caller, sig, GuestPtr::new(0x140), GuestPtrMut::NULL, 8);
    assert_eq!(block_on(set), Ok(0));
}

/// `CLONE_SIGHAND` without `CLONE_THREAD` shares the signal handlers between the two
/// processes (`sighand_struct`): a `sigaction` of either changes them for both, while their
/// pending signals and masks stay their own. Without it the child has a copy. An `execve`
/// gives its process handlers of its own again (`unshare_sighand`).
#[test]
fn clone_sighand_shares_the_handlers_between_processes() {
    let (kernel, task, _) = setup(b"");
    let memory = big_forkable();
    let caller = Caller::new(task, &memory);
    assert_eq!(block_on(kernel.signal_callback_register(&caller, 3)), Ok(0));
    set_handler(&kernel, &caller, Signal::SIGUSR1, 7);
    let shared = VM | SIGCHLD | CloneFlags::CLONE_SIGHAND.0 as u32;
    let child = TaskId(spawn(&kernel, &caller, shared).unwrap() as u32);
    let copied = TaskId(spawn(&kernel, &caller, VM | SIGCHLD).unwrap() as u32);
    while kernel.take_pending(task).is_some() {}
    let (child, copied) = (Caller::new(child, &memory), Caller::new(copied, &memory));

    assert_eq!(handler(&kernel, &child, Signal::SIGUSR1), 7);
    set_handler(&kernel, &child, Signal::SIGUSR2, 9);
    assert_eq!(
        handler(&kernel, &caller, Signal::SIGUSR2),
        9,
        "the parent's too"
    );
    set_handler(&kernel, &caller, Signal::SIGUSR1, 1);
    assert_eq!(handler(&kernel, &child, Signal::SIGUSR1), 1);
    assert_eq!(handler(&kernel, &copied, Signal::SIGUSR1), 7, "a copy");
    assert_eq!(handler(&kernel, &copied, Signal::SIGUSR2), 0);
    // Their pending signals are their own.
    assert_eq!(
        block_on(kernel.sys_kill(&caller, child.task().0 as Pid, Signal::SIGUSR2)),
        Ok(0)
    );
    assert!(
        kernel
            .task_signals(child.task())
            .has_pending(1 << (Signal::SIGUSR2.0 - 1))
    );
    assert!(
        !kernel
            .task_signals(task)
            .has_pending(1 << (Signal::SIGUSR2.0 - 1))
    );

    let child_memory = FlatMemory(RefCell::new(vec![0; 65536]));
    let execing = Caller::new(child.task(), &child_memory);
    super::task::install_prog(&kernel, &execing, &child_memory);
    while kernel.take_pending(child.task()).is_some() {}
    assert_eq!(block_on(super::task::execve(&kernel, &execing)), Ok(0));
    set_handler(&kernel, &caller, Signal::SIGUSR2, 4);
    assert_eq!(
        handler(&kernel, &execing, Signal::SIGUSR2),
        0,
        "its own, reset"
    );
    assert_eq!(handler(&kernel, &caller, Signal::SIGUSR2), 4);
}

/// kernel/exit.c kernel_waitid_prepare/kernel_waitid: O_NONBLOCK supplies WNOHANG,
/// but only an implicit WNOHANG reports EAGAIN. No ready child leaves rusage untouched.
#[test]
fn waitid_honors_pidfd_nonblock_and_only_copies_ready_rusage() {
    let (kernel, task, _) = setup(b"");
    let memory = ForkableMemory(FlatMemory(RefCell::new(vec![0; 4096])));
    let caller = Caller::new(task, &memory);
    let child = fork_child(&kernel, &caller);
    let fd = block_on(kernel.pidfd_open(&caller, child.0 as Pid, OpenFlags::O_NONBLOCK.0)).unwrap();
    put(&memory.0, 0x100, &[0xaa; 128]);
    put(&memory.0, 0x200, &[0xaa; 144]);
    let wait = kernel.waitid(
        &caller,
        Idtype::P_PIDFD,
        fd.0,
        SiginfoPtr::new(0x100),
        WaitOptions::WEXITED,
        RusagePtr::new(0x200),
    );
    assert_eq!(wait.now_or_never(), Some(Err(SysErr::Errno(Errno::EAGAIN))));
    assert_eq!(word(&memory.0, 0x100), 0, "si_signo");
    assert_eq!(super::ipc::get(&memory.0, 0x200, 144), vec![0xaa; 144]);
    assert_eq!(
        block_on(kernel.waitid(
            &caller,
            Idtype::P_PIDFD,
            fd.0,
            SiginfoPtr::new(0xffff_ffff),
            WaitOptions::WEXITED,
            RusagePtr::NULL,
        )),
        Err(SysErr::Errno(Errno::EFAULT)),
        "siginfo copy faults override EAGAIN"
    );
    assert_eq!(
        block_on(kernel.waitid(
            &caller,
            Idtype::P_PIDFD,
            -1,
            SiginfoPtr::NULL,
            WaitOptions::WEXITED,
            RusagePtr::NULL,
        )),
        Err(SysErr::Errno(Errno::EINVAL))
    );
    let wait = kernel.waitid(
        &caller,
        Idtype::P_PIDFD,
        fd.0,
        SiginfoPtr::new(0x100),
        WaitOptions::WEXITED | WaitOptions::WNOHANG,
        RusagePtr::new(0xffff_ffff),
    );
    assert_eq!(
        block_on(wait),
        Ok(0),
        "rusage is not accessed without an event"
    );
    assert_eq!(
        block_on(kernel.fcntl64(&caller, fd, wasixv2_abi::FcntlCmd::F_SETFL, 0)),
        Ok(0)
    );
    let wait = kernel.waitid(
        &caller,
        Idtype::P_PIDFD,
        fd.0,
        SiginfoPtr::new(0x100),
        WaitOptions::WEXITED,
        RusagePtr::NULL,
    );
    assert!(
        wait.now_or_never().is_none(),
        "F_SETFL changed the live file flags"
    );
    assert_eq!(
        block_on(kernel.fcntl64(
            &caller,
            fd,
            wasixv2_abi::FcntlCmd::F_SETFL,
            OpenFlags::O_NONBLOCK.0
        )),
        Ok(0)
    );
    let child_memory = forkable();
    assert_eq!(
        block_on(kernel.exit_group(&Caller::new(child, &child_memory), 7)),
        Ok(())
    );
    assert_eq!(
        block_on(kernel.waitid(
            &caller,
            Idtype::P_PIDFD,
            fd.0,
            SiginfoPtr::new(0x100),
            WaitOptions::WEXITED,
            RusagePtr::new(0x200)
        )),
        Ok(0)
    );
    assert_eq!(word(&memory.0, 0x100), Signal::SIGCHLD.0 as u32);
    assert_eq!(word(&memory.0, 0x100 + 12), child.0);
    assert_eq!(word(&memory.0, 0x100 + 20), 7);
    assert_ne!(super::ipc::get(&memory.0, 0x200, 144), vec![0xaa; 144]);
}
