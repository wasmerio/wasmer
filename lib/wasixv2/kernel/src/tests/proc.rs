//! fork: clone without `CLONE_VM`.

use wasixv2_abi::{
    CloneFlags, ConstRlimit64Ptr, Idtype, Pid, Rlimit64Ptr, RlimitResource, RusagePtr, S32Ptr,
    SiginfoPtr, Signal, WaitOptions,
};

use super::*;

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
            parent_tid: Some(8),
            child_tid: Some(16),
            tls: None,
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
fn fork_rejects_a_new_stack_and_clone_vfork() {
    let (kernel, task, _) = setup(b"");
    let memory = forkable();
    let caller = Caller::new(task, &memory);

    let stack = clone(&kernel, &caller, SIGCHLD, 4096);
    let blocking = clone(
        &kernel,
        &caller,
        SIGCHLD | CloneFlags::CLONE_VFORK.0 as u32,
        0,
    );

    assert_eq!(stack, Err(SysErr::Errno(Errno::EINVAL)));
    assert_eq!(blocking, Err(SysErr::Errno(Errno::EINVAL)));
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
