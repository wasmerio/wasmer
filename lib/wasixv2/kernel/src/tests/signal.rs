//! Signals: the alternate stack, `RLIMIT_SIGPENDING`, the parent-death signal, pidfds, and how
//! a thread group's end is reported.

use std::cell::Cell;

use futures::FutureExt as _;
use wasixv2_abi::{
    ClockId, CloneFlags, ConstRlimit64Ptr, ConstSiginfoPtr, Idtype, KernelTimespec, PrctlOption,
    Rlimit64Ptr, RlimitResource, RusagePtr, SaFlags, Sigaction, Sigaltstack, SigaltstackFlags,
    Siginfo, SiginfoPtr, Signal, SigprocmaskHow, Sigset, WaitOptions,
};
use wasixv2_syscalls::{SignalFrame, VforkParent, read_value, write_value};

use super::{cred::machine_as, ipc::machine, proc::ForkableMemory, *};

/// Memory of a task whose stack pointer the test moves, as the guest's `__stack_pointer`.
struct StackMemory {
    memory: FlatMemory,
    sp: Cell<u32>,
}

impl UserMemory for StackMemory {
    fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        self.memory.read(addr, buf)
    }

    fn write<'a>(&'a self, addr: u32, data: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        self.memory.write(addr, data)
    }

    fn size(&self) -> LocalFuture<'_, u64> {
        self.memory.size()
    }

    fn grow(&self, pages: u32) -> LocalFuture<'_, Option<u64>> {
        self.memory.grow(pages)
    }

    fn stack_pointer(&self) -> LocalFuture<'_, u32> {
        let sp = self.sp.get();
        Box::pin(async move { sp })
    }
}

/// An address space the engine can duplicate, as the asyncify configuration's is.
fn forkable() -> ForkableMemory {
    ForkableMemory(FlatMemory(RefCell::new(vec![0; 0x1000])))
}

fn stack_memory() -> StackMemory {
    StackMemory {
        memory: FlatMemory(RefCell::new(vec![0; 0x10000])),
        sp: Cell::new(NORMAL_SP),
    }
}

/// Where the normal stack runs, and the alternate stack the tests set.
const NORMAL_SP: u32 = 0x1000;
const ALT_SP: u32 = 0x8000;
const ALT_SIZE: u32 = 0x2000;
const ALT_TOP: u32 = ALT_SP + ALT_SIZE;
const AUTODISARM: u32 = SigaltstackFlags::SS_AUTODISARM.0;
const SIGRTMIN: i32 = 32;

fn err<T>(errno: Errno) -> SysResult<T> {
    Err(SysErr::Errno(errno))
}

fn set_altstack(
    kernel: &Kernel,
    caller: &Caller<'_>,
    sp: u32,
    size: u32,
    flags: u32,
) -> SysResult<i32> {
    let ss = Sigaltstack {
        ss_sp: sp,
        ss_flags: flags as i32,
        ss_size: size,
    };
    block_on(write_value(caller.memory(), GuestPtrMut::new(0x100), &ss)).unwrap();
    block_on(kernel.sigaltstack(caller, GuestPtr::new(0x100), GuestPtrMut::NULL))
}

fn altstack(kernel: &Kernel, caller: &Caller<'_>) -> (u32, u32, u32) {
    let got = block_on(kernel.sigaltstack(caller, GuestPtr::NULL, GuestPtrMut::new(0x120)));
    assert_eq!(got, Ok(0));
    let ss: Sigaltstack = block_on(read_value(caller.memory(), GuestPtr::new(0x120))).unwrap();
    (ss.ss_sp, ss.ss_flags as u32, ss.ss_size)
}

/// Installs handler 7 for `sig` with `flags`, behind trampoline 3.
fn handle(kernel: &Kernel, caller: &Caller<'_>, sig: Signal, flags: u32) {
    let act = Sigaction {
        sa_handler: 7,
        sa_flags: flags,
        sa_mask: Sigset { sig: [0; 2] },
    };
    block_on(write_value(caller.memory(), GuestPtrMut::new(0x140), &act)).unwrap();
    assert_eq!(block_on(kernel.signal_callback_register(caller, 3)), Ok(0));
    let set = kernel.rt_sigaction(caller, sig, GuestPtr::new(0x140), GuestPtrMut::NULL, 8);
    assert_eq!(block_on(set), Ok(0));
}

/// Sends `sig` to the caller's process and takes the handler frame it leads to.
fn deliver(kernel: &Kernel, caller: &Caller<'_>, sig: Signal) -> SignalFrame {
    let me = kernel.process(caller.task()).pid.0 as Pid;
    assert_eq!(block_on(kernel.sys_kill(caller, me, sig)), Ok(0));
    match kernel.take_pending(caller.task()) {
        Some(GuestAction::Signal(frame)) => frame,
        other => panic!("expected a handler frame, got {other:?}"),
    }
}

fn sigset(mask: u64) -> Sigset {
    Sigset {
        sig: [mask as u32, (mask >> 32) as u32],
    }
}

fn block(kernel: &Kernel, caller: &Caller<'_>, mask: u64) {
    block_on(write_value(
        caller.memory(),
        GuestPtrMut::new(0x160),
        &sigset(mask),
    ))
    .unwrap();
    let how = SigprocmaskHow::SIG_BLOCK;
    let set = kernel.rt_sigprocmask(caller, how, GuestPtr::new(0x160), GuestPtrMut::NULL, 8);
    assert_eq!(block_on(set), Ok(0));
}

/// `sigtimedwait` for `sig` without waiting: the `siginfo` it takes, if one is pending.
fn take(kernel: &Kernel, caller: &Caller<'_>, sig: i32) -> Option<Siginfo> {
    let these = sigset(1 << (sig - 1));
    let now = KernelTimespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    block_on(write_value(
        caller.memory(),
        GuestPtrMut::new(0x170),
        &these,
    ))
    .unwrap();
    block_on(write_value(caller.memory(), GuestPtrMut::new(0x178), &now)).unwrap();
    let wait = kernel.rt_sigtimedwait_time64(
        caller,
        GuestPtr::new(0x170),
        GuestPtrMut::new(0x200),
        GuestPtr::new(0x178),
        8,
    );
    match block_on(wait) {
        Ok(got) => {
            assert_eq!(got, sig);
            Some(block_on(read_value(caller.memory(), GuestPtr::new(0x200))).unwrap())
        }
        Err(SysErr::Errno(Errno::EAGAIN)) => None,
        Err(other) => panic!("sigtimedwait failed: {other:?}"),
    }
}

fn sifield(info: &Siginfo, at: usize) -> u32 {
    u32::from_le_bytes(info.sifields[at..at + 4].try_into().unwrap())
}

fn clone(kernel: &Kernel, caller: &Caller<'_>, flags: u32) -> SysResult<i32> {
    let flags = flags | Signal::SIGCHLD.0 as u32;
    block_on(kernel.clone(caller, flags, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL))
}

/// Forks the caller and returns the child.
fn fork(kernel: &Kernel, caller: &Caller<'_>) -> TaskId {
    let child = TaskId(clone(kernel, caller, 0).expect("fork") as u32);
    while kernel.take_pending(caller.task()).is_some() {}
    child
}

const VFORK: u32 = CloneFlags::CLONE_VM.0 as u32 | CloneFlags::CLONE_VFORK.0 as u32;

/// vforks the caller and returns the child, which its instance runs as from now on.
fn vfork(kernel: &Kernel, caller: &Caller<'_>) -> TaskId {
    assert_eq!(clone(kernel, caller, VFORK), Ok(0));
    loop {
        match kernel.take_pending(caller.task()) {
            Some(GuestAction::SwitchTask { task, .. }) => return task,
            Some(_) => {}
            None => panic!("vfork left no task switch"),
        }
    }
}

/// `waitid(P_PID, pid, WEXITED | WNOHANG)`: the child's `si_status` once it is a zombie.
fn reap(kernel: &Kernel, caller: &Caller<'_>, pid: TaskId) -> Option<u32> {
    let options = WaitOptions(WaitOptions::WEXITED.0 | WaitOptions::WNOHANG.0);
    let wait = kernel.waitid(
        caller,
        Idtype::P_PID,
        pid.0 as Pid,
        SiginfoPtr::new(0x300),
        options,
        RusagePtr::new(0),
    );
    assert_eq!(block_on(wait), Ok(0));
    let info: Siginfo = block_on(read_value(caller.memory(), GuestPtr::new(0x300))).unwrap();
    (info.si_signo != 0).then(|| sifield(&info, 8))
}

const THREAD: u32 = 0x10f00;

fn spawn_thread(kernel: &Kernel, caller: &Caller<'_>) -> TaskId {
    let spawn = kernel.thread_spawn(caller, THREAD, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL);
    let tid = block_on(spawn).unwrap();
    while kernel.take_pending(caller.task()).is_some() {}
    TaskId(tid as u32)
}

// ---- sigaltstack ----

#[test]
fn the_alternate_stack_is_in_use_where_the_stack_pointer_is() {
    let (kernel, task, _) = machine();
    let memory = stack_memory();
    let caller = Caller::new(task, &memory);
    assert_eq!(set_altstack(&kernel, &caller, ALT_SP, ALT_SIZE, 0), Ok(0));
    assert_eq!(altstack(&kernel, &caller), (ALT_SP, 0, ALT_SIZE));

    // `on_sig_stack`: above the base, up to and including the top.
    for (sp, on) in [
        (ALT_SP, false),
        (ALT_SP + 16, true),
        (ALT_TOP, true),
        (ALT_TOP + 4, false),
    ] {
        memory.sp.set(sp);
        let flags = if on {
            SigaltstackFlags::SS_ONSTACK.0
        } else {
            0
        };
        assert_eq!(altstack(&kernel, &caller).1, flags, "at {sp:#x}");
        let change = set_altstack(&kernel, &caller, ALT_SP, ALT_SIZE / 2, 0);
        assert_eq!(change, if on { err(Errno::EPERM) } else { Ok(0) });
        if !on {
            assert_eq!(set_altstack(&kernel, &caller, ALT_SP, ALT_SIZE, 0), Ok(0));
        }
    }

    memory.sp.set(NORMAL_SP);
    let disable = SigaltstackFlags::SS_DISABLE.0;
    assert_eq!(
        set_altstack(&kernel, &caller, ALT_SP, 16, 0),
        err(Errno::ENOMEM)
    );
    assert_eq!(
        set_altstack(&kernel, &caller, ALT_SP, ALT_SIZE, 7),
        err(Errno::EINVAL)
    );
    assert_eq!(set_altstack(&kernel, &caller, ALT_SP, 16, disable), Ok(0));
    assert_eq!(altstack(&kernel, &caller), (0, disable, 0));
}

#[test]
fn a_handler_runs_on_the_alternate_stack_unless_the_guest_is_on_it_already() {
    let (kernel, task, _) = machine();
    let memory = stack_memory();
    let caller = Caller::new(task, &memory);
    assert_eq!(set_altstack(&kernel, &caller, ALT_SP, ALT_SIZE, 0), Ok(0));
    handle(&kernel, &caller, Signal::SIGUSR1, SaFlags::SA_ONSTACK.0);
    handle(&kernel, &caller, Signal::SIGUSR2, 0);

    let frame = deliver(&kernel, &caller, Signal::SIGUSR1);
    assert_eq!(frame.altstack, (ALT_SP, 0, ALT_SIZE), "uc_stack");
    assert_eq!(frame.altstack_top(NORMAL_SP), Some(ALT_TOP));
    assert_eq!(frame.altstack_top(ALT_SP + 64), None, "nested on the stack");
    let frame = deliver(&kernel, &caller, Signal::SIGUSR2);
    assert_eq!(frame.altstack_top(NORMAL_SP), None, "no SA_ONSTACK");
}

#[test]
fn a_handler_that_switches_away_leaves_the_alternate_stack_usable() {
    let (kernel, task, _) = machine();
    let memory = stack_memory();
    let caller = Caller::new(task, &memory);
    assert_eq!(set_altstack(&kernel, &caller, ALT_SP, ALT_SIZE, 0), Ok(0));
    handle(
        &kernel,
        &caller,
        Signal::SIGUSR1,
        SaFlags::SA_ONSTACK.0 | SaFlags::SA_NODEFER.0,
    );

    // The handler runs on the alternate stack, then switches to a coroutine on the normal
    // stack and never returns.
    let frame = deliver(&kernel, &caller, Signal::SIGUSR1);
    assert_eq!(frame.altstack_top(NORMAL_SP), Some(ALT_TOP));
    memory.sp.set(NORMAL_SP);

    assert_eq!(altstack(&kernel, &caller), (ALT_SP, 0, ALT_SIZE));
    assert_eq!(set_altstack(&kernel, &caller, ALT_SP, ALT_SIZE, 0), Ok(0));
    let frame = deliver(&kernel, &caller, Signal::SIGUSR1);
    assert_eq!(frame.altstack_top(NORMAL_SP), Some(ALT_TOP));
}

#[test]
fn ss_autodisarm_disarms_the_stack_for_the_handler_and_sigreturn_rearms_it() {
    let (kernel, task, _) = machine();
    let memory = stack_memory();
    let caller = Caller::new(task, &memory);
    assert_eq!(
        set_altstack(&kernel, &caller, ALT_SP, ALT_SIZE, AUTODISARM),
        Ok(0)
    );
    assert_eq!(altstack(&kernel, &caller), (ALT_SP, AUTODISARM, ALT_SIZE));
    handle(&kernel, &caller, Signal::SIGUSR1, SaFlags::SA_ONSTACK.0);

    let frame = deliver(&kernel, &caller, Signal::SIGUSR1);
    assert_eq!(frame.altstack, (ALT_SP, AUTODISARM as i32, ALT_SIZE));
    assert_eq!(frame.altstack_top(NORMAL_SP), Some(ALT_TOP));
    // In the handler, on the alternate stack: it is disarmed, and may be set again.
    memory.sp.set(ALT_TOP - 256);
    let disable = SigaltstackFlags::SS_DISABLE.0;
    assert_eq!(altstack(&kernel, &caller), (0, disable, 0));
    assert_eq!(
        set_altstack(&kernel, &caller, 0xc000, ALT_SIZE, AUTODISARM),
        Ok(0)
    );
    // sigreturn makes it what the frame saved.
    memory.sp.set(NORMAL_SP);
    kernel.signal_return(task, Some(0));
    assert_eq!(altstack(&kernel, &caller), (ALT_SP, AUTODISARM, ALT_SIZE));

    // A handler that `siglongjmp`s out restores nothing.
    deliver(&kernel, &caller, Signal::SIGUSR1);
    kernel.signal_return(task, None);
    assert_eq!(altstack(&kernel, &caller), (0, disable, 0));
}

#[test]
fn a_fork_or_vfork_child_keeps_the_alternate_stack_and_a_thread_starts_without() {
    let (kernel, task, _) = machine();
    let memory = forkable();
    let caller = Caller::new(task, &memory);
    assert_eq!(set_altstack(&kernel, &caller, ALT_SP, ALT_SIZE, 0), Ok(0));

    let thread = spawn_thread(&kernel, &caller);
    let forked = fork(&kernel, &caller);
    let vforked = vfork(&kernel, &caller);

    let disabled = (0, SigaltstackFlags::SS_DISABLE.0, 0);
    assert_eq!(altstack(&kernel, &Caller::new(thread, &memory)), disabled);
    let inherited = (ALT_SP, 0, ALT_SIZE);
    assert_eq!(altstack(&kernel, &Caller::new(forked, &memory)), inherited);
    assert_eq!(altstack(&kernel, &Caller::new(vforked, &memory)), inherited);
}

// ---- RLIMIT_SIGPENDING ----

fn set_sigpending_limit(kernel: &Kernel, caller: &Caller<'_>, limit: u64) {
    let rlimit = [limit.to_le_bytes(), limit.to_le_bytes()].concat();
    block_on(caller.memory().write(0x180, &rlimit)).unwrap();
    let set = kernel.prlimit64(
        caller,
        0,
        RlimitResource::RLIMIT_SIGPENDING,
        ConstRlimit64Ptr::new(0x180),
        Rlimit64Ptr::new(0),
    );
    assert_eq!(block_on(set), Ok(0));
}

/// The `siginfo` `sigqueue` sends: `SI_QUEUE` with `si_value`.
fn queue_info(sig: i32, value: u32) -> Siginfo {
    let mut info = Siginfo {
        si_signo: sig,
        si_errno: 0,
        si_code: wasixv2_abi::SiCode::SI_QUEUE.0,
        sifields: [0; 116],
    };
    info.sifields[8..12].copy_from_slice(&value.to_le_bytes());
    info
}

fn sigqueue(
    kernel: &Kernel,
    caller: &Caller<'_>,
    pid: Pid,
    sig: i32,
    value: u32,
) -> SysResult<i32> {
    let info = queue_info(sig, value);
    block_on(write_value(caller.memory(), GuestPtrMut::new(0x400), &info)).unwrap();
    block_on(kernel.rt_sigqueueinfo(caller, pid, Signal(sig), GuestPtr::new(0x400)))
}

#[test]
fn realtime_signals_queue_up_to_rlimit_sigpending() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let me = task.0 as Pid;
    set_sigpending_limit(&kernel, &caller, 2);
    block(&kernel, &caller, 1 << (SIGRTMIN - 1));

    assert_eq!(sigqueue(&kernel, &caller, me, SIGRTMIN, 1), Ok(0));
    assert_eq!(sigqueue(&kernel, &caller, me, SIGRTMIN, 2), Ok(0));
    assert_eq!(
        sigqueue(&kernel, &caller, me, SIGRTMIN, 3),
        err(Errno::EAGAIN)
    );
    // The timer's preallocated signal counts too.
    let timer = kernel.timer_create(
        &caller,
        ClockId::CLOCK_MONOTONIC,
        GuestPtr::NULL,
        GuestPtrMut::new(0x500),
    );
    assert_eq!(block_on(timer), err(Errno::EAGAIN));
    // `kill` is never refused: the signal is pending without its `siginfo`.
    assert_eq!(
        block_on(kernel.sys_kill(&caller, me, Signal(SIGRTMIN))),
        Ok(0)
    );

    let first = take(&kernel, &caller, SIGRTMIN).unwrap();
    assert_eq!(first.si_code, wasixv2_abi::SiCode::SI_QUEUE.0);
    assert_eq!(sifield(&first, 8), 1, "si_value");
    // Taking one makes room for one.
    assert_eq!(sigqueue(&kernel, &caller, me, SIGRTMIN, 4), Ok(0));
    assert_eq!(
        sigqueue(&kernel, &caller, me, SIGRTMIN, 5),
        err(Errno::EAGAIN)
    );
    let values: Vec<u32> = std::iter::from_fn(|| take(&kernel, &caller, SIGRTMIN))
        .map(|info| sifield(&info, 8))
        .collect();
    assert_eq!(values, [2, 4]);
    assert_eq!(take(&kernel, &caller, SIGRTMIN), None);
}

#[test]
fn a_signal_without_room_for_its_siginfo_comes_from_pid_zero() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let me = task.0 as Pid;
    set_sigpending_limit(&kernel, &caller, 0);
    block(&kernel, &caller, 1 << (SIGRTMIN - 1));

    assert_eq!(
        block_on(kernel.sys_kill(&caller, me, Signal(SIGRTMIN))),
        Ok(0)
    );

    let info = take(&kernel, &caller, SIGRTMIN).unwrap();
    assert_eq!(info.si_code, wasixv2_abi::SiCode::SI_USER.0);
    assert_eq!(
        (sifield(&info, 0), sifield(&info, 4)),
        (0, 0),
        "si_pid, si_uid"
    );
}

// ---- PR_SET_PDEATHSIG ----

fn set_pdeathsig(kernel: &Kernel, caller: &Caller<'_>, sig: Signal) {
    let prctl = kernel.prctl(caller, PrctlOption::PR_SET_PDEATHSIG, sig.0 as u32, 0, 0, 0);
    assert_eq!(block_on(prctl), Ok(0));
}

fn pdeathsig(kernel: &Kernel, caller: &Caller<'_>) -> i32 {
    let prctl = kernel.prctl(caller, PrctlOption::PR_GET_PDEATHSIG, 0x190, 0, 0, 0);
    assert_eq!(block_on(prctl), Ok(0));
    block_on(read_value(caller.memory(), GuestPtr::<i32>::new(0x190))).unwrap()
}

#[test]
fn the_parent_death_signal_comes_when_the_parent_exits() {
    let (kernel, task, _) = machine();
    let memory = forkable();
    let caller = Caller::new(task, &memory);
    let child = fork(&kernel, &caller);
    let child_memory = forkable();
    let child_caller = Caller::new(child, &child_memory);
    set_pdeathsig(&kernel, &child_caller, Signal::SIGUSR1);
    block(&kernel, &child_caller, 1 << (Signal::SIGUSR1.0 - 1));
    assert_eq!(take(&kernel, &child_caller, Signal::SIGUSR1.0), None);

    assert_eq!(block_on(kernel.exit_group(&caller, 0)), Ok(()));

    let info = take(&kernel, &child_caller, Signal::SIGUSR1.0).expect("SIGUSR1");
    assert_eq!(info.si_code, wasixv2_abi::SiCode::SI_USER.0);
    assert_eq!(sifield(&info, 0), task.0, "si_pid");
    assert_eq!(block_on(kernel.getppid(&child_caller)), Ok(0));
}

#[test]
fn changing_credentials_forgets_the_parent_death_signal() {
    let (kernel, task, memory) = machine_as(0, 0);
    let caller = Caller::new(task, &memory);
    set_pdeathsig(&kernel, &caller, Signal::SIGTERM);
    assert_eq!(pdeathsig(&kernel, &caller), Signal::SIGTERM.0);
    // The real uid alone keeps it; the effective one forgets it.
    assert_eq!(
        block_on(kernel.setresuid(&caller, 5, u32::MAX, u32::MAX)),
        Ok(0)
    );
    assert_eq!(pdeathsig(&kernel, &caller), Signal::SIGTERM.0);
    assert_eq!(block_on(kernel.setuid(&caller, 5)), Ok(0));
    assert_eq!(pdeathsig(&kernel, &caller), 0);
}

// ---- thread group ends ----

#[test]
fn a_group_whose_leader_exited_first_ends_with_the_leaders_status() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let thread = spawn_thread(&kernel, &caller);
    let thread_caller = Caller::new(thread, &memory);

    // The leader waits as a zombie until the last thread exits.
    let mut leader = Box::pin(kernel.exit(&caller, 5));
    assert_eq!(leader.as_mut().now_or_never(), None);
    assert_eq!(block_on(kernel.exit(&thread_caller, 7)), Ok(()));
    assert_eq!(leader.as_mut().now_or_never(), Some(Ok(())));

    assert_eq!(kernel.take_pending(task), Some(GuestAction::Exit(5)));
    assert_eq!(kernel.take_pending(thread), Some(GuestAction::Exit(5)));
}

/// A process with a second thread, forked from `init`, whose instance runs its vfork child.
struct Vforked {
    kernel: Kernel,
    init: TaskId,
    init_memory: ForkableMemory,
    memory: FlatMemory,
    parent: TaskId,
    thread: TaskId,
    child: TaskId,
}

impl Vforked {
    fn new() -> Self {
        let (kernel, init, _) = machine();
        let init_memory = forkable();
        let parent = fork(&kernel, &Caller::new(init, &init_memory));
        let memory = FlatMemory(RefCell::new(vec![0; 0x1000]));
        let caller = Caller::new(parent, &memory);
        let thread = spawn_thread(&kernel, &caller);
        let child = vfork(&kernel, &caller);
        Self {
            kernel,
            init,
            init_memory,
            memory,
            parent,
            thread,
            child,
        }
    }

    /// The parent's other thread ends the group with `status` and unwinds.
    fn exit_group(&self, status: i32) {
        let caller = Caller::new(self.thread, &self.memory);
        assert_eq!(block_on(self.kernel.exit_group(&caller, status)), Ok(()));
        assert_eq!(
            self.kernel.take_pending(self.thread),
            Some(GuestAction::Exit(status))
        );
        self.kernel.release(self.thread);
    }

    fn reap_parent(&self) -> Option<u32> {
        let caller = Caller::new(self.init, &self.init_memory);
        reap(&self.kernel, &caller, self.parent)
    }

    /// What the instance goes back to once the child exited.
    fn child_exits(&self) -> GuestAction {
        let caller = Caller::new(self.child, &self.memory);
        assert_eq!(block_on(self.kernel.exit_group(&caller, 0)), Ok(()));
        self.kernel.take_pending(self.child).unwrap()
    }
}

#[test]
fn a_vfork_parent_whose_group_exits_is_no_zombie_while_its_child_runs() {
    let vforked = Vforked::new();
    let Vforked {
        kernel,
        parent,
        child,
        ..
    } = &vforked;
    let (parent, child) = (*parent, *child);

    // Another thread ends the group while the parent's instance runs the child.
    vforked.exit_group(3);
    assert_eq!(vforked.reap_parent(), None);

    // The child's end hands the instance back, and the parent unwinds through its exit
    // instead of returning from vfork.
    let exited = VforkParent {
        task: parent,
        resumes: false,
    };
    assert_eq!(
        vforked.child_exits(),
        GuestAction::VforkReturn {
            parent: exited,
            child
        }
    );
    kernel.release(child);
    assert_eq!(vforked.reap_parent(), None);
    kernel.release(parent);
    assert_eq!(vforked.reap_parent(), Some(3));
}

#[test]
fn a_vfork_parent_whose_group_exits_does_not_return_from_its_childs_execve() {
    const IMAGE: &[u8] = b"\0asm\x01\0\0\0";
    let vforked = Vforked::new();
    let Vforked {
        kernel,
        memory,
        parent,
        child,
        ..
    } = &vforked;
    let caller = Caller::new(*child, memory);
    put(memory, 0x100, b"/prog\0");
    put(memory, 0x110, IMAGE);
    let create = OpenFlags::O_CREAT | OpenFlags::O_WRONLY;
    let fd = block_on(kernel.openat(&caller, Fd(-100), Cstr::new(0x100), create, 0o755)).unwrap();
    let written = block_on(kernel.write(&caller, fd, UserSlice::new(0x110, IMAGE.len() as u32)));
    assert_eq!(written, Ok(IMAGE.len() as u32));
    assert_eq!(block_on(kernel.close(&caller, fd)), Ok(0));
    // argv = { "/prog", NULL }, envp = { NULL }.
    put(memory, 0x120, &0x100u32.to_le_bytes());

    vforked.exit_group(3);
    let execve = kernel.execve(
        &caller,
        Cstr::new(0x100),
        GuestPtr::new(0x120),
        GuestPtr::new(0x124),
    );
    assert_eq!(block_on(execve), Ok(0));

    // The new image runs on its own; the instance unwinds the parent through its exit.
    let vfork_parent = Some(VforkParent {
        task: *parent,
        resumes: false,
    });
    assert_eq!(
        kernel.take_pending(*child),
        Some(GuestAction::Exec { vfork_parent })
    );
    kernel.release(*parent);
    assert_eq!(vforked.reap_parent(), Some(3));
}

/// The exit that counts is the one pending when the instance goes back to the parent: one
/// that came after the child's end was queued kills the parent all the same.
#[test]
fn a_vfork_parent_whose_group_exits_after_its_child_ended_does_not_return() {
    let vforked = Vforked::new();
    let caller = Caller::new(vforked.child, &vforked.memory);
    assert_eq!(block_on(vforked.kernel.exit_group(&caller, 0)), Ok(()));

    vforked.exit_group(4);

    let exited = VforkParent {
        task: vforked.parent,
        resumes: false,
    };
    assert_eq!(
        vforked.kernel.take_pending(vforked.child),
        Some(GuestAction::VforkReturn {
            parent: exited,
            child: vforked.child
        })
    );
}

#[test]
fn a_vfork_parent_with_no_exit_pending_returns_from_vfork() {
    let vforked = Vforked::new();
    let parent = VforkParent {
        task: vforked.parent,
        resumes: true,
    };
    assert_eq!(
        vforked.child_exits(),
        GuestAction::VforkReturn {
            parent,
            child: vforked.child
        }
    );
}

// ---- clone and pidfds ----

#[test]
fn clone_pidfd_is_refused() {
    let (kernel, task, _) = machine();
    let memory = forkable();
    let caller = Caller::new(task, &memory);
    let pidfd = CloneFlags::CLONE_PIDFD.0 as u32;

    assert_eq!(clone(&kernel, &caller, pidfd), err(Errno::EINVAL));
    assert_eq!(clone(&kernel, &caller, VFORK | pidfd), err(Errno::EINVAL));
    assert_eq!(kernel.take_pending(task), None);
}

#[test]
fn pidfd_send_signal_passes_its_siginfo_and_reaches_a_zombie() {
    let (kernel, task, _) = machine();
    let memory = forkable();
    let caller = Caller::new(task, &memory);
    let child = fork(&kernel, &caller);
    let child_memory = forkable();
    let child_caller = Caller::new(child, &child_memory);
    block(&kernel, &child_caller, 1 << (SIGRTMIN - 1));
    let pidfd = block_on(kernel.pidfd_open(&caller, child.0 as Pid, 0)).unwrap();
    let send = |sig: i32, info: u32| {
        let info = ConstSiginfoPtr::new(info);
        block_on(kernel.pidfd_send_signal(&caller, pidfd, Signal(sig), info, 0))
    };
    let mut info = queue_info(SIGRTMIN, 42);
    block_on(write_value(&memory, GuestPtrMut::new(0x400), &info)).unwrap();

    assert_eq!(send(SIGRTMIN, 0x400), Ok(0));
    let got = take(&kernel, &child_caller, SIGRTMIN).unwrap();
    assert_eq!(got.si_code, wasixv2_abi::SiCode::SI_QUEUE.0);
    assert_eq!(sifield(&got, 8), 42);
    assert_eq!(
        send(SIGRTMIN + 1, 0x400),
        err(Errno::EINVAL),
        "si_signo differs"
    );
    info.si_code = wasixv2_abi::SiCode::SI_USER.0;
    block_on(write_value(&memory, GuestPtrMut::new(0x400), &info)).unwrap();
    assert_eq!(
        send(SIGRTMIN, 0x400),
        err(Errno::EPERM),
        "the kernel's code"
    );

    assert_eq!(block_on(kernel.exit_group(&child_caller, 0)), Ok(()));
    assert_eq!(send(Signal::SIGTERM.0, 0), Ok(0), "a zombie");
    assert_eq!(reap(&kernel, &caller, child), Some(0));
    assert_eq!(send(Signal::SIGTERM.0, 0), err(Errno::ESRCH), "reaped");
}
