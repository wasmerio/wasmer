//! Signals: the alternate stack.

use std::cell::Cell;

use wasixv2_abi::{CloneFlags, SaFlags, Sigaction, Sigaltstack, SigaltstackFlags, Signal, Sigset};
use wasixv2_syscalls::{SignalFrame, read_value, write_value};

use super::{ipc::machine, proc::ForkableMemory, *};

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
