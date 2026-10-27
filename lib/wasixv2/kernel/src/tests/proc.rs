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
    assert!(kernel.process(task).exe.lock().unwrap().is_none());
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

    thread::forget_task(&mut kernel.state.lock().unwrap(), leader);
    kernel.exit_files(&process);
}

/// Records what `execve` shows a format handler, and refuses what it is told to.
struct Recording {
    refuse_header: bool,
    seen: Mutex<Vec<(&'static str, usize)>>,
}

impl wasixv2_syscalls::Binfmt for Recording {
    fn probe(&self, header: &[u8]) -> Result<(), Errno> {
        self.seen.lock().unwrap().push(("probe", header.len()));
        match self.refuse_header {
            true => Err(Errno::ENOEXEC),
            false => Ok(()),
        }
    }

    fn check(&self, image: &wasixv2_syscalls::Image) -> Result<(), Errno> {
        self.seen
            .lock()
            .unwrap()
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
        let seen = binfmt.seen.lock().unwrap().clone();
        match refuse_header {
            true => assert_eq!(seen, [("probe", BINPRM_BUF_SIZE)]),
            false => assert_eq!(seen, [("probe", BINPRM_BUF_SIZE), ("check", image.len())]),
        }
    }
}
