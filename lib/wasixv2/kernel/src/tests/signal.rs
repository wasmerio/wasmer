//! Signals: the alternate stack, `RLIMIT_SIGPENDING`, the parent-death signal, pidfds, and how
//! a thread group's end is reported.

use std::cell::Cell;

use futures::FutureExt as _;
use wasixv2_abi::{
    ClockId, CloneFlags, ConstRlimit64Ptr, ConstSiginfoPtr, Idtype, KernelTimespec, PrctlOption,
    Rlimit64Ptr, RlimitResource, RusagePtr, SaFlags, Sigaction, Sigaltstack, SigaltstackFlags,
    Siginfo, SiginfoPtr, Signal, SigprocmaskHow, Sigset, WaitOptions,
};
use wasixv2_syscalls::{Resume, SignalFrame, VforkParent, read_value, write_value};
use wasixv2_wait::Interrupt as _;

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

/// `wait_for_vfork_done` is killable: a parent whose group another thread ends is a zombie
/// as soon as it left its memory, which the child's instance does for it at the child's next
/// return to the guest, while the instance goes on running the child, which goes to the
/// reaper.
#[test]
fn a_vfork_parent_whose_group_exits_is_a_zombie_while_its_child_runs() {
    let vforked = Vforked::new();
    let Vforked {
        kernel,
        memory,
        parent,
        child,
        ..
    } = &vforked;
    let (parent, child) = (*parent, *child);
    kernel.work_flag(child).clear();

    // Another thread ends the group while the parent's instance runs the child.
    vforked.exit_group(3);
    assert!(
        kernel.work_flag(child).is_raised(),
        "the child leaves its wait"
    );
    assert_eq!(
        vforked.reap_parent(),
        None,
        "the parent still has its memory"
    );
    let dead = GuestAction::ExitMm { task: parent };
    assert_eq!(kernel.take_pending(child), Some(dead));
    block_on(kernel.exit_mm(&Caller::new(parent, memory)));
    assert_eq!(kernel.take_pending(child), None, "once");
    assert_eq!(vforked.reap_parent(), Some(3));
    let child_caller = Caller::new(child, memory);
    assert_eq!(
        block_on(kernel.getppid(&child_caller)),
        Ok(vforked.init.0 as Pid)
    );

    // The child's end hands the instance back, and the parent unwinds through its exit
    // instead of returning from vfork.
    let exited = VforkParent {
        task: parent,
        resumes: Resume::Dies,
    };
    assert_eq!(
        vforked.child_exits(),
        GuestAction::VforkReturn {
            parent: exited,
            child
        }
    );
    kernel.release(child);
    assert_eq!(kernel.take_pending(parent), Some(GuestAction::Exit(3)));
    kernel.release(parent);
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
        resumes: Resume::Dies,
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
        resumes: Resume::Dies,
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
        resumes: Resume::Returns,
    };
    assert_eq!(
        vforked.child_exits(),
        GuestAction::VforkReturn {
            parent,
            child: vforked.child
        }
    );
}

/// A vfork child that a signal kills hands the instance back to its parent, as its exit does:
/// the kill must not unwind the parent's stack, which the child runs on. Its process is seen to
/// exit at once, as its memory is the parent's.
#[test]
fn a_vfork_child_killed_by_a_signal_hands_the_instance_back_to_its_parent() {
    let vforked = Vforked::new();
    let (kernel, child) = (&vforked.kernel, vforked.child);
    let caller = Caller::new(child, &vforked.memory);
    let kill = kernel.sys_kill(&caller, child.0 as Pid, Signal::SIGKILL);
    assert_eq!(block_on(kill), Ok(0));

    let parent = VforkParent {
        task: vforked.parent,
        resumes: Resume::Returns,
    };
    assert_eq!(
        kernel.take_pending(child),
        Some(GuestAction::VforkReturn { parent, child })
    );
}

/// A process forked from `init` whose single thread waits for its vfork child.
struct Parked {
    kernel: Kernel,
    init: TaskId,
    init_memory: ForkableMemory,
    memory: FlatMemory,
    parent: TaskId,
    child: TaskId,
}

impl Parked {
    fn new() -> Self {
        let (kernel, init, _) = machine();
        let init_memory = forkable();
        let parent = fork(&kernel, &Caller::new(init, &init_memory));
        let memory = FlatMemory(RefCell::new(vec![0; 0x1000]));
        let child = vfork(&kernel, &Caller::new(parent, &memory));
        Self {
            kernel,
            init,
            init_memory,
            memory,
            parent,
            child,
        }
    }

    fn init_caller(&self) -> Caller<'_> {
        Caller::new(self.init, &self.init_memory)
    }

    /// `waitid(P_PID, parent, WEXITED | WNOHANG)` from `init`: `si_code` and `si_status`.
    fn reap_parent(&self) -> Option<(i32, u32)> {
        let caller = self.init_caller();
        let status = reap(&self.kernel, &caller, self.parent)?;
        let info: Siginfo = block_on(read_value(&self.init_memory, GuestPtr::new(0x300))).unwrap();
        Some((info.si_code, status))
    }

    /// Whether the child's instance was told to look at its work.
    fn child_notified(&self) -> bool {
        self.kernel.work_flag(self.child).is_raised()
    }
}

/// `wait_for_vfork_done` is `TASK_KILLABLE`: SIGKILL to a parent waiting for its vfork child
/// kills it at the child's next return to the guest, where the instance leaves the memory for
/// it. Its parent reaps it while the child goes on in its instance, which unwinds it once the
/// child is done.
#[test]
fn a_vfork_parent_killed_while_its_child_runs_dies_at_once() {
    let parked = Parked::new();
    let Parked {
        kernel,
        memory,
        parent,
        child,
        ..
    } = &parked;
    let (parent, child) = (*parent, *child);
    let child_caller = Caller::new(child, memory);
    kernel.work_flag(child).clear();

    let init_caller = parked.init_caller();
    let kill = kernel.sys_kill(&init_caller, parent.0 as Pid, Signal::SIGKILL);
    assert_eq!(block_on(kill), Ok(0));
    assert!(parked.child_notified(), "the child leaves its wait");
    assert_eq!(parked.reap_parent(), None, "nothing ran for the child yet");

    let dead = GuestAction::ExitMm { task: parent };
    assert_eq!(kernel.take_pending(child), Some(dead));
    assert_eq!(
        parked.reap_parent(),
        None,
        "the parent still has its memory"
    );
    block_on(kernel.exit_mm(&Caller::new(parent, memory)));
    assert_eq!(kernel.take_pending(child), None, "the child goes on");
    let killed = wasixv2_abi::SiCode::CLD_KILLED.0;
    assert_eq!(parked.reap_parent(), Some((killed, 9)));
    assert_eq!(
        block_on(kernel.getppid(&child_caller)),
        Ok(parked.init.0 as Pid)
    );

    let exited = VforkParent {
        task: parent,
        resumes: Resume::Dies,
    };
    assert_eq!(block_on(kernel.exit_group(&child_caller, 0)), Ok(()));
    assert_eq!(
        kernel.take_pending(child),
        Some(GuestAction::VforkReturn {
            parent: exited,
            child
        })
    );
    kernel.release(child);
    assert_eq!(
        kernel.take_pending(parent),
        Some(GuestAction::Exit(128 + 9))
    );
    kernel.release(parent);
}

/// A vfork parent killed while its child runs leaves its memory there and then, as Linux's
/// does in its own `do_exit` (`exit_mm`), on the memory the child goes on running on: its
/// robust futexes go to their waiters, and its `clear_child_tid` is zeroed, since the child
/// still uses the memory (`mm_release`). Only then is it a zombie. Its instance comes back to
/// it once the child is done, to unwind it, which leaves nothing more.
#[test]
fn a_vfork_parent_killed_while_its_child_runs_releases_its_robust_futexes() {
    let parked = Parked::new();
    let Parked {
        kernel,
        memory,
        parent,
        child,
        ..
    } = &parked;
    let (parent, child) = (*parent, *child);
    let parent_caller = Caller::new(parent, memory);
    let child_caller = Caller::new(child, memory);
    // The parent's robust list holds one mutex it owns, the word 4 bytes into the entry.
    const HEAD: u32 = 0x200;
    const ENTRY: u32 = 0x210;
    const TID: u32 = 0x220;
    put(memory, HEAD, &ENTRY.to_le_bytes());
    put(memory, HEAD + 4, &4u32.to_le_bytes());
    put(memory, ENTRY, &HEAD.to_le_bytes());
    put(memory, ENTRY + 4, &parent.0.to_le_bytes());
    let head = wasixv2_abi::RobustListHeadPtr::new(HEAD);
    let set = kernel.set_robust_list(&parent_caller, head, 12);
    assert_eq!(block_on(set), Ok(0));
    put(memory, TID, &parent.0.to_le_bytes());
    let tid = block_on(kernel.set_tid_address(&parent_caller, S32Ptr::new(TID)));
    assert_eq!(tid, Ok(parent.0 as i32));

    let init_caller = parked.init_caller();
    let kill = kernel.sys_kill(&init_caller, parent.0 as Pid, Signal::SIGKILL);
    assert_eq!(block_on(kill), Ok(0));
    let dead = GuestAction::ExitMm { task: parent };
    assert_eq!(kernel.take_pending(child), Some(dead));
    assert_eq!(word(memory, ENTRY + 4), parent.0, "not before its exit_mm");
    block_on(kernel.exit_mm(&parent_caller));
    assert_eq!(word(memory, ENTRY + 4), 0x4000_0000, "FUTEX_OWNER_DIED");
    assert_eq!(word(memory, TID), 0, "clear_child_tid");
    let killed = wasixv2_abi::SiCode::CLD_KILLED.0;
    assert_eq!(parked.reap_parent(), Some((killed, 9)));

    // The child's end, as the execution layer runs it: the child leaves the memory, the
    // instance goes back to the parent, which takes its exit and has nothing more to leave.
    put(memory, ENTRY + 4, &child.0.to_le_bytes());
    assert_eq!(block_on(kernel.exit_group(&child_caller, 0)), Ok(()));
    let exited = VforkParent {
        task: parent,
        resumes: Resume::Dies,
    };
    assert_eq!(
        kernel.take_pending(child),
        Some(GuestAction::VforkReturn {
            parent: exited,
            child
        })
    );
    block_on(kernel.exit_mm(&child_caller));
    kernel.release(child);
    assert_eq!(
        kernel.take_pending(parent),
        Some(GuestAction::Exit(128 + 9))
    );
    block_on(kernel.exit_mm(&parent_caller));
    assert_eq!(word(memory, ENTRY + 4), child.0, "released once");
    kernel.release(parent);
    // Reaped once: the later exits do not make the parent a zombie again.
    let options = WaitOptions(WaitOptions::WEXITED.0 | WaitOptions::WNOHANG.0);
    let wait = kernel.waitid(
        &init_caller,
        Idtype::P_PID,
        parent.0 as Pid,
        SiginfoPtr::new(0x300),
        options,
        RusagePtr::new(0),
    );
    assert_eq!(block_on(wait), Err(Errno::ECHILD.into()));
}

/// Only a signal that kills the parent ends its wait: one it ignores, blocks or handles waits
/// until the parent runs again.
#[test]
fn a_vfork_parent_keeps_waiting_through_signals_that_do_not_kill_it() {
    for case in ["ignored", "blocked", "handled"] {
        let parked = Parked::new();
        let Parked {
            kernel,
            memory,
            parent,
            child,
            ..
        } = &parked;
        let parent_caller = Caller::new(*parent, memory);
        match case {
            "ignored" => {
                let act = Sigaction {
                    sa_handler: wasixv2_abi::SignalHandler::SIG_IGN.0 as u32,
                    sa_flags: 0,
                    sa_mask: sigset(0),
                };
                block_on(write_value(memory, GuestPtrMut::new(0x140), &act)).unwrap();
                let set = kernel.rt_sigaction(
                    &parent_caller,
                    Signal::SIGTERM,
                    GuestPtr::new(0x140),
                    GuestPtrMut::NULL,
                    8,
                );
                assert_eq!(block_on(set), Ok(0));
            }
            "blocked" => block(kernel, &parent_caller, 1 << (Signal::SIGTERM.0 - 1)),
            _ => handle(kernel, &parent_caller, Signal::SIGTERM, 0),
        }
        kernel.work_flag(*child).clear();

        let init_caller = parked.init_caller();
        let kill = kernel.sys_kill(&init_caller, parent.0 as Pid, Signal::SIGTERM);
        assert_eq!(block_on(kill), Ok(0));

        assert!(!parked.child_notified(), "{case}");
        assert_eq!(kernel.take_pending(*child), None, "{case}");
        assert_eq!(parked.reap_parent(), None, "{case}");
        let resumes = VforkParent {
            task: *parent,
            resumes: Resume::Returns,
        };
        let child_caller = Caller::new(*child, memory);
        assert_eq!(block_on(kernel.exit_group(&child_caller, 0)), Ok(()));
        assert_eq!(
            kernel.take_pending(*child),
            Some(GuestAction::VforkReturn {
                parent: resumes,
                child: *child
            }),
            "{case}"
        );
    }
}

/// A thread other than the leader waiting for its vfork child does not keep its killed group
/// from becoming a zombie either, once the child's instance left its memory for it.
#[test]
fn a_killed_group_does_not_wait_for_a_thread_waiting_for_its_vfork_child() {
    let (kernel, init, _) = machine();
    let init_memory = forkable();
    let init_caller = Caller::new(init, &init_memory);
    let parent = fork(&kernel, &init_caller);
    let memory = FlatMemory(RefCell::new(vec![0; 0x1000]));
    let thread = spawn_thread(&kernel, &Caller::new(parent, &memory));
    let child = vfork(&kernel, &Caller::new(thread, &memory));

    let kill = kernel.sys_kill(&init_caller, parent.0 as Pid, Signal::SIGKILL);
    assert_eq!(block_on(kill), Ok(0));
    assert_eq!(
        kernel.take_pending(parent),
        Some(GuestAction::Exit(128 + 9))
    );
    kernel.release(parent);
    assert_eq!(reap(&kernel, &init_caller, parent), None);
    let dead = GuestAction::ExitMm { task: thread };
    assert_eq!(kernel.take_pending(child), Some(dead));
    block_on(kernel.exit_mm(&Caller::new(thread, &memory)));
    assert_eq!(reap(&kernel, &init_caller, parent), Some(9));

    let child_caller = Caller::new(child, &memory);
    assert_eq!(block_on(kernel.exit_group(&child_caller, 0)), Ok(()));
    let exited = VforkParent {
        task: thread,
        resumes: Resume::Dies,
    };
    assert_eq!(
        kernel.take_pending(child),
        Some(GuestAction::VforkReturn {
            parent: exited,
            child
        })
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

/// Makes the caller's process ignore `sig`.
pub(super) fn ignore(kernel: &Kernel, caller: &Caller<'_>, sig: Signal) {
    let act = Sigaction {
        sa_handler: wasixv2_abi::SignalHandler::SIG_IGN.0 as u32,
        sa_flags: 0,
        sa_mask: Sigset { sig: [0; 2] },
    };
    block_on(write_value(caller.memory(), GuestPtrMut::new(0x140), &act)).unwrap();
    let set = kernel.rt_sigaction(caller, sig, GuestPtr::new(0x140), GuestPtrMut::NULL, 8);
    assert_eq!(block_on(set), Ok(0));
}

/// A terminal's signal to a process group says whether some process has it pending: one that
/// every process ignores is discarded where it is generated, one that a process handles, or
/// blocks though it ignores it, is pending.
#[test]
fn a_process_group_signal_says_whether_it_is_pending() {
    let (kernel, task, _) = machine();
    let memory = forkable();
    let caller = Caller::new(task, &memory);
    let child = fork(&kernel, &caller);
    let child_memory = forkable();
    let child_caller = Caller::new(child, &child_memory);
    let sigint = Signal::SIGINT;
    ignore(&kernel, &caller, sigint);
    ignore(&kernel, &child_caller, sigint);

    assert_eq!(
        kernel.kill_process_group(task, sigint),
        Ok(false),
        "ignored"
    );
    assert!(!kernel.process_group_pending(task, sigint));

    block(&kernel, &child_caller, 1 << (sigint.0 - 1));
    assert_eq!(
        kernel.kill_process_group(child, sigint),
        Ok(true),
        "blocked"
    );
    assert!(kernel.process_group_pending(task, sigint));
    assert!(take(&kernel, &child_caller, sigint.0).is_some());

    handle(&kernel, &caller, sigint, 0);
    assert_eq!(kernel.kill_process_group(task, sigint), Ok(true), "handled");
    assert!(matches!(
        kernel.take_pending(task),
        Some(GuestAction::Signal(_))
    ));
}

/// `terminate` kills every process, children included, and returns once the execution layer
/// has released all their tasks.
#[test]
fn terminate_kills_every_process_and_waits_for_their_tasks() {
    let (kernel, task, _) = machine();
    let memory = forkable();
    let caller = Caller::new(task, &memory);
    let child = fork(&kernel, &caller);

    let mut terminate = std::pin::pin!(kernel.terminate());
    assert!(
        terminate.as_mut().now_or_never().is_none(),
        "tasks are left"
    );
    for task in [task, child] {
        assert!(matches!(
            kernel.take_pending(task),
            Some(GuestAction::Exit(_))
        ));
    }
    kernel.release_task(child);
    assert!(
        terminate.as_mut().now_or_never().is_none(),
        "one task is left"
    );
    kernel.release_task(task);
    assert_eq!(terminate.now_or_never(), Some(()));
}

// ---- a signal before the call that would hide it ----

/// A process signal that only a thread on its way out takes (`pthread_exit` blocks every
/// signal first) reaches that thread before its mask hides it: the `sigprocmask` makes way for
/// the handler and runs again after it, as Linux's kick of a running task would have run the
/// handler before the call. Otherwise nothing takes the signal (`pthread_once/6-1` hung on it,
/// its sender waiting for the handler forever).
#[test]
fn sigprocmask_lets_a_pending_signal_it_would_block_be_taken_first() {
    let (kernel, task, _) = machine();
    let memory = stack_memory();
    let caller = Caller::new(task, &memory);
    let usr1 = 1 << (Signal::SIGUSR1.0 - 1);
    handle(&kernel, &caller, Signal::SIGUSR1, 0);
    let thread = spawn_thread(&kernel, &caller);
    let thread_caller = Caller::new(thread, &memory);
    block(&kernel, &caller, usr1);
    let me = task.0 as Pid;
    assert_eq!(
        block_on(kernel.sys_kill(&caller, me, Signal::SIGUSR1)),
        Ok(0)
    );

    block_on(write_value(&memory, GuestPtrMut::new(0x160), &sigset(usr1))).unwrap();
    let block_usr1 = || {
        let how = SigprocmaskHow::SIG_BLOCK;
        let set = GuestPtr::new(0x160);
        block_on(kernel.rt_sigprocmask(&thread_caller, how, set, GuestPtrMut::NULL, 8))
    };
    assert_eq!(block_usr1(), Err(SysErr::RestartNoIntr));
    assert!(kernel.work_flag(thread).is_raised());
    let frame = match kernel.take_pending(thread) {
        Some(GuestAction::Signal(frame)) => frame,
        other => panic!("expected the handler first, got {other:?}"),
    };
    assert_eq!(frame.signo(), Signal::SIGUSR1.0);
    kernel.signal_return(thread, Some(frame.mask));

    assert_eq!(block_usr1(), Ok(0), "nothing pending any more");
    assert_eq!(kernel.task_signals(thread).blocked(), usr1);
    // A signal sent once the mask hides it waits, as on Linux.
    assert_eq!(
        block_on(kernel.sys_kill(&caller, me, Signal::SIGUSR1)),
        Ok(0)
    );
    assert_eq!(block_usr1(), Ok(0));
}

/// The same for `sigaction` setting `SIG_IGN`, which discards the pending signal: the handler
/// it replaces takes the signal first.
#[test]
fn ignoring_a_signal_the_caller_could_take_lets_its_handler_run_first() {
    let (kernel, task, _) = machine();
    let memory = stack_memory();
    let caller = Caller::new(task, &memory);
    handle(&kernel, &caller, Signal::SIGUSR1, 0);
    let me = task.0 as Pid;
    assert_eq!(
        block_on(kernel.sys_kill(&caller, me, Signal::SIGUSR1)),
        Ok(0)
    );

    let act = Sigaction {
        sa_handler: wasixv2_abi::SignalHandler::SIG_IGN.0 as u32,
        sa_flags: 0,
        sa_mask: Sigset { sig: [0; 2] },
    };
    block_on(write_value(&memory, GuestPtrMut::new(0x180), &act)).unwrap();
    let set_ignore = || {
        let set = GuestPtr::new(0x180);
        block_on(kernel.rt_sigaction(&caller, Signal::SIGUSR1, set, GuestPtrMut::NULL, 8))
    };
    assert_eq!(set_ignore(), Err(SysErr::RestartNoIntr));
    assert!(matches!(
        kernel.take_pending(task),
        Some(GuestAction::Signal(SignalFrame { handler: 7, .. }))
    ));
    kernel.signal_return(task, Some(0));
    assert_eq!(set_ignore(), Ok(0));

    // Blocked, the signal is the caller's to discard.
    let usr1 = 1 << (Signal::SIGUSR1.0 - 1);
    block(&kernel, &caller, usr1);
    handle(&kernel, &caller, Signal::SIGUSR1, 0);
    assert_eq!(
        block_on(kernel.sys_kill(&caller, me, Signal::SIGUSR1)),
        Ok(0)
    );
    assert_eq!(set_ignore(), Ok(0));
    assert!(take(&kernel, &caller, Signal::SIGUSR1.0).is_none());
}

/// A signal for the whole group goes to the one task Linux's `complete_signal` picks, the
/// leader when it takes it: no other task leaves its wait for it or takes it on its way back to
/// the guest. LTP's library sends a heartbeat SIGUSR1 to its own process, and `tgkill01`'s
/// handler, which records the thread it runs on, then ran on a thread waiting in a futex, late
/// enough to look like a `tgkill` delivered to the wrong thread.
#[test]
fn a_group_signal_is_taken_by_the_leader_alone() {
    let (kernel, task, _) = machine();
    let memory = stack_memory();
    let caller = Caller::new(task, &memory);
    handle(&kernel, &caller, Signal::SIGUSR1, 0);
    let thread = spawn_thread(&kernel, &caller);
    while kernel.take_pending(thread).is_some() {}

    let me = task.0 as Pid;
    assert_eq!(
        block_on(kernel.sys_kill(&caller, me, Signal::SIGUSR1)),
        Ok(0)
    );
    assert!(
        !kernel.task_signals(thread).pending(),
        "the thread goes on waiting"
    );
    assert_eq!(
        kernel.take_pending(thread),
        None,
        "the signal is the leader's"
    );
    match kernel.take_pending(task) {
        Some(GuestAction::Signal(frame)) => assert_eq!(frame.signo(), Signal::SIGUSR1.0),
        other => panic!("expected the leader's handler, got {other:?}"),
    }
}

/// A group signal the leader blocks goes to the next thread that takes it, and only to that
/// one. A thread that exits before it took the signal hands it on (`retarget_shared_pending`).
#[test]
fn a_group_signal_the_leader_blocks_goes_to_one_thread_and_on_when_it_exits() {
    let (kernel, task, _) = machine();
    let memory = stack_memory();
    let caller = Caller::new(task, &memory);
    handle(&kernel, &caller, Signal::SIGUSR1, 0);
    let first = spawn_thread(&kernel, &caller);
    let second = spawn_thread(&kernel, &caller);
    block(&kernel, &caller, 1 << (Signal::SIGUSR1.0 - 1));

    let me = task.0 as Pid;
    assert_eq!(
        block_on(kernel.sys_kill(&caller, me, Signal::SIGUSR1)),
        Ok(0)
    );
    assert!(kernel.task_signals(first).pending());
    assert!(!kernel.task_signals(second).pending());
    assert_eq!(kernel.take_pending(second), None);

    let first_caller = Caller::new(first, &memory);
    assert_eq!(block_on(kernel.exit(&first_caller, 0)), Ok(()));
    assert_eq!(kernel.take_pending(first), Some(GuestAction::Exit(0)));
    assert!(kernel.task_signals(second).pending(), "handed on");
    match kernel.take_pending(second) {
        Some(GuestAction::Signal(frame)) => assert_eq!(frame.signo(), Signal::SIGUSR1.0),
        other => panic!("expected the handler on the second thread, got {other:?}"),
    }
}
