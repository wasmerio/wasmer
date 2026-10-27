//! Signals: the alternate stack, `RLIMIT_SIGPENDING` and the parent-death signal.

use std::cell::Cell;

use wasixv2_abi::{
    ClockId, CloneFlags, ConstRlimit64Ptr, KernelTimespec, PrctlOption, Rlimit64Ptr,
    RlimitResource, SaFlags, Sigaction, Sigaltstack, SigaltstackFlags, Siginfo, Signal,
    SigprocmaskHow, Sigset,
};
use wasixv2_syscalls::{SignalFrame, read_value, write_value};

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
