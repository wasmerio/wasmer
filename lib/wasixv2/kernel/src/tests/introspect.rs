//! Looking at and into other processes: `kcmp`, `pidfd_getfd`, Yama's ptrace scope and
//! `PR_SET_PTRACER`, I/O priorities, and the memory calls that only check or count (`mseal`,
//! `cachestat`, `process_madvise`), and `vhangup`.

use wasixv2_abi::{
    Cachestat, CachestatRange, EpollCreateFlags, EpollCtlOp, EpollEvent, FcntlCmd, IoprioClass,
    IoprioWho, KcmpEpollSlot, KcmpType, MadviseAdvice, MapFlags, PrctlOption, ProtFlags,
};
use wasixv2_syscalls::{read_value, write_value};

use super::{
    cred::{machine_as, set_caps},
    ipc::{errno, pipe, put},
    proc::{ForkableMemory, fork_child},
    *,
};

/// 64 KiB of memory a fork can duplicate.
fn memory() -> ForkableMemory {
    ForkableMemory(FlatMemory(RefCell::new(vec![0; 65536])))
}

fn kcmp(
    kernel: &Kernel,
    caller: &Caller<'_>,
    p1: TaskId,
    p2: TaskId,
    ty: KcmpType,
) -> SysResult<i32> {
    kcmp_idx(kernel, caller, (p1, 0), (p2, 0), ty)
}

fn kcmp_idx(
    kernel: &Kernel,
    caller: &Caller<'_>,
    (p1, idx1): (TaskId, u32),
    (p2, idx2): (TaskId, u32),
    ty: KcmpType,
) -> SysResult<i32> {
    block_on(kernel.kcmp(caller, p1.0 as i32, p2.0 as i32, ty.0, idx1, idx2))
}

const RESOURCES: [KcmpType; 6] = [
    KcmpType::KCMP_VM,
    KcmpType::KCMP_FILES,
    KcmpType::KCMP_FS,
    KcmpType::KCMP_SIGHAND,
    KcmpType::KCMP_IO,
    KcmpType::KCMP_SYSVSEM,
];

/// `kcmp`: a task shares everything with itself; a fork child shares no table with its parent,
/// but the files it inherited; the order of two different resources reverses with the
/// arguments.
#[test]
fn kcmp_compares_what_tasks_share() {
    let (kernel, parent, _) = setup(b"");
    let memory = memory();
    let caller = Caller::new(parent, &memory);
    let (rd, wr) = pipe(&kernel, &caller, OpenFlags::empty());
    let child = fork_child(&kernel, &caller);

    for ty in RESOURCES {
        assert_eq!(kcmp(&kernel, &caller, parent, parent, ty), Ok(0), "{ty:?}");
    }
    for ty in [
        KcmpType::KCMP_VM,
        KcmpType::KCMP_FILES,
        KcmpType::KCMP_FS,
        KcmpType::KCMP_SIGHAND,
    ] {
        let there = kcmp(&kernel, &caller, parent, child, ty).unwrap();
        let back = kcmp(&kernel, &caller, child, parent, ty).unwrap();
        assert!(
            matches!((there, back), (1, 2) | (2, 1)),
            "{ty:?}: {there} {back}"
        );
    }
    // Neither set an I/O priority or has semaphore adjustments: both have none, which is equal.
    assert_eq!(
        kcmp(&kernel, &caller, parent, child, KcmpType::KCMP_IO),
        Ok(0)
    );
    assert_eq!(
        kcmp(&kernel, &caller, parent, child, KcmpType::KCMP_SYSVSEM),
        Ok(0)
    );

    let file = KcmpType::KCMP_FILE;
    let same = kcmp_idx(
        &kernel,
        &caller,
        (parent, rd.0 as u32),
        (child, rd.0 as u32),
        file,
    );
    assert_eq!(same, Ok(0));
    let other = kcmp_idx(
        &kernel,
        &caller,
        (parent, rd.0 as u32),
        (child, wr.0 as u32),
        file,
    );
    assert!(matches!(other, Ok(1 | 2)));
    let closed = kcmp_idx(&kernel, &caller, (parent, 99), (child, rd.0 as u32), file);
    assert_eq!(errno(closed), Errno::EBADF);

    assert_eq!(
        errno(kcmp(&kernel, &caller, parent, parent, KcmpType(8))),
        Errno::EINVAL
    );
    assert_eq!(
        errno(kcmp(&kernel, &caller, TaskId(0), parent, KcmpType::KCMP_VM)),
        Errno::ESRCH
    );
    assert_eq!(
        errno(kcmp(
            &kernel,
            &caller,
            TaskId(999),
            parent,
            KcmpType::KCMP_VM
        )),
        Errno::ESRCH
    );
}

/// `KCMP_EPOLL_TFD`: a file against the target an epoll watches with a descriptor number, the
/// `toff`-th of those.
#[test]
fn kcmp_finds_a_file_an_epoll_watches() {
    let (kernel, task, _) = setup(b"");
    let memory = memory();
    let caller = Caller::new(task, &memory);
    let (rd, _wr) = pipe(&kernel, &caller, OpenFlags::empty());
    let epfd = block_on(kernel.epoll_create1(&caller, EpollCreateFlags::empty())).unwrap();
    let event = EpollEvent::default();
    block_on(write_value(&memory, GuestPtrMut::new(0x300), &event)).unwrap();
    let ctl = kernel.epoll_ctl(
        &caller,
        epfd,
        EpollCtlOp::EPOLL_CTL_ADD,
        rd,
        GuestPtr::new(0x300),
    );
    assert_eq!(block_on(ctl), Ok(0));

    let slot = |toff: u32| {
        let slot = KcmpEpollSlot {
            efd: epfd.0 as u32,
            tfd: rd.0 as u32,
            toff,
        };
        block_on(write_value(&memory, GuestPtrMut::new(0x400), &slot)).unwrap();
        kcmp_idx(
            &kernel,
            &caller,
            (task, rd.0 as u32),
            (task, 0x400),
            KcmpType::KCMP_EPOLL_TFD,
        )
    };
    assert_eq!(slot(0), Ok(0));
    assert_eq!(errno(slot(1)), Errno::ENOENT);
    // The epoll descriptor must be one.
    let slot = KcmpEpollSlot {
        efd: rd.0 as u32,
        tfd: rd.0 as u32,
        toff: 0,
    };
    block_on(write_value(&memory, GuestPtrMut::new(0x400), &slot)).unwrap();
    let not_epoll = kcmp_idx(
        &kernel,
        &caller,
        (task, rd.0 as u32),
        (task, 0x400),
        KcmpType::KCMP_EPOLL_TFD,
    );
    assert_eq!(errno(not_epoll), Errno::EINVAL);
}

/// `kcmp` needs `PTRACE_MODE_READ_REALCREDS` over both tasks: an unprivileged process may not
/// look into root's.
#[test]
fn kcmp_of_a_more_privileged_process_is_eperm() {
    let (kernel, root, _) = machine_as(0, 0);
    let memory = memory();
    let caller = Caller::new(root, &memory);
    let child = fork_child(&kernel, &caller);
    let child_memory = memory_flat();
    let child_caller = Caller::new(child, &child_memory);
    assert_eq!(
        block_on(kernel.setresuid(&child_caller, 1000, 1000, 1000)),
        Ok(0)
    );

    let vm = KcmpType::KCMP_VM;
    assert_eq!(
        errno(kcmp(&kernel, &child_caller, child, root, vm)),
        Errno::EPERM
    );
    assert!(kcmp(&kernel, &caller, child, root, vm).is_ok());
}

fn memory_flat() -> FlatMemory {
    FlatMemory(RefCell::new(vec![0; 65536]))
}

/// `pidfd_getfd`: a close-on-exec duplicate of another process's open file, for a caller that
/// may attach to it; Yama keeps a child from attaching to its parent unless the parent named it
/// with `PR_SET_PTRACER`.
#[test]
fn pidfd_getfd_duplicates_a_file_of_a_process_one_may_attach_to() {
    let (kernel, parent, _) = setup(b"");
    let memory = memory();
    let caller = Caller::new(parent, &memory);
    let child = fork_child(&kernel, &caller);
    let child_memory = memory_flat();
    let child_caller = Caller::new(child, &child_memory);
    let (rd, _wr) = pipe(&kernel, &child_caller, OpenFlags::empty());

    let pidfd = block_on(kernel.pidfd_open(&caller, child.0 as i32, 0)).unwrap();
    let fd = block_on(kernel.pidfd_getfd(&caller, pidfd, rd, 0)).unwrap();
    let same = kcmp_idx(
        &kernel,
        &caller,
        (parent, fd as u32),
        (child, rd.0 as u32),
        KcmpType::KCMP_FILE,
    );
    assert_eq!(same, Ok(0));
    let flags = block_on(kernel.fcntl64(&caller, Fd(fd), FcntlCmd::F_GETFD, 0)).unwrap();
    assert_eq!(flags, 1, "FD_CLOEXEC");

    assert_eq!(
        errno(block_on(kernel.pidfd_getfd(&caller, pidfd, rd, 1))),
        Errno::EINVAL
    );
    assert_eq!(
        errno(block_on(kernel.pidfd_getfd(&caller, pidfd, Fd(77), 0))),
        Errno::EBADF
    );
    assert_eq!(
        errno(block_on(kernel.pidfd_getfd(&caller, Fd(fd), rd, 0))),
        Errno::EBADF
    );

    // The child going the other way: its parent is no descendant of it.
    let up = block_on(kernel.pidfd_open(&child_caller, parent.0 as i32, 0)).unwrap();
    let (prd, _pwr) = pipe(&kernel, &caller, OpenFlags::empty());
    let attach = || block_on(kernel.pidfd_getfd(&child_caller, up, prd, 0));
    assert_eq!(errno(attach()), Errno::EPERM);
    let ptracer = kernel.prctl(&caller, PrctlOption::PR_SET_PTRACER, child.0, 0, 0, 0);
    assert_eq!(block_on(ptracer), Ok(0));
    assert!(attach().is_ok());
    let forget = kernel.prctl(&caller, PrctlOption::PR_SET_PTRACER, 0, 0, 0, 0);
    assert_eq!(block_on(forget), Ok(0));
    assert_eq!(errno(attach()), Errno::EPERM);
    let any = kernel.prctl(&caller, PrctlOption::PR_SET_PTRACER, u32::MAX, 0, 0, 0);
    assert_eq!(block_on(any), Ok(0));
    assert!(attach().is_ok());
    let nobody = kernel.prctl(&caller, PrctlOption::PR_SET_PTRACER, 4242, 0, 0, 0);
    assert_eq!(errno(block_on(nobody)), Errno::EINVAL);
}

/// With `ptrace_scope` 0 Yama adds nothing; with 3 nobody attaches, not even to a child.
#[test]
fn the_ptrace_scope_decides_who_attaches() {
    let (kernel, parent, _) = setup(b"");
    let memory = memory();
    let caller = Caller::new(parent, &memory);
    let child = fork_child(&kernel, &caller);
    let child_memory = memory_flat();
    let child_caller = Caller::new(child, &child_memory);
    let (rd, _wr) = pipe(&kernel, &caller, OpenFlags::empty());
    let up = block_on(kernel.pidfd_open(&child_caller, parent.0 as i32, 0)).unwrap();
    let down = block_on(kernel.pidfd_open(&caller, child.0 as i32, 0)).unwrap();

    kernel.yama.set_scope(0).unwrap();
    assert!(block_on(kernel.pidfd_getfd(&child_caller, up, rd, 0)).is_ok());
    kernel.yama.set_scope(3).unwrap();
    assert_eq!(
        errno(block_on(kernel.pidfd_getfd(&caller, down, rd, 0))),
        Errno::EPERM
    );
}

fn ioprio_get(kernel: &Kernel, caller: &Caller<'_>, who: IoprioWho, id: i32) -> SysResult<i32> {
    block_on(kernel.ioprio_get(caller, who.0, id))
}

fn ioprio(class: IoprioClass, level: i32) -> i32 {
    class.0 << 13 | level
}

/// `ioprio_set` and `ioprio_get`: a process reads the priority it set, `IOPRIO_DEFAULT` before
/// that; a group's best priority counts what each nice value gives a task that set none; fork
/// children inherit a priority that was set.
#[test]
fn io_priorities_are_set_read_and_inherited() {
    let (kernel, task, _) = setup(b"");
    let memory = memory();
    let caller = Caller::new(task, &memory);
    let process = IoprioWho::IOPRIO_WHO_PROCESS;
    assert_eq!(ioprio_get(&kernel, &caller, process, 0), Ok(0));
    // A group whose tasks set nothing: best effort, at nice 0's level 4.
    let be4 = ioprio(IoprioClass::IOPRIO_CLASS_BE, 4);
    assert_eq!(
        ioprio_get(&kernel, &caller, IoprioWho::IOPRIO_WHO_PGRP, 0),
        Ok(be4)
    );

    let idle = ioprio(IoprioClass::IOPRIO_CLASS_IDLE, 0);
    assert_eq!(
        block_on(kernel.ioprio_set(&caller, process.0, 0, idle)),
        Ok(0)
    );
    assert_eq!(ioprio_get(&kernel, &caller, process, 0), Ok(idle));
    let child = fork_child(&kernel, &caller);
    assert_eq!(
        ioprio_get(&kernel, &caller, process, child.0 as i32),
        Ok(idle)
    );
    // Each has a context of its own: no `CLONE_IO`.
    assert!(matches!(
        kcmp(&kernel, &caller, task, child, KcmpType::KCMP_IO),
        Ok(1 | 2)
    ));

    let set =
        |prio: i32, which: i32, who: i32| block_on(kernel.ioprio_set(&caller, which, who, prio));
    let rt = ioprio(IoprioClass::IOPRIO_CLASS_RT, 0);
    assert_eq!(
        errno(set(rt, process.0, 0)),
        Errno::EPERM,
        "RT needs CAP_SYS_NICE"
    );
    assert_eq!(
        errno(set(ioprio(IoprioClass::IOPRIO_CLASS_NONE, 1), process.0, 0)),
        Errno::EINVAL
    );
    assert_eq!(
        errno(set(
            ioprio(IoprioClass::IOPRIO_CLASS_INVALID, 0),
            process.0,
            0
        )),
        Errno::EINVAL
    );
    assert_eq!(errno(set(idle, 9, 0)), Errno::EINVAL);
    assert_eq!(errno(set(idle, process.0, 4242)), Errno::ESRCH);
    assert_eq!(
        errno(ioprio_get(&kernel, &caller, IoprioWho(0), 0)),
        Errno::EINVAL
    );
}

/// Another user's task needs `CAP_SYS_NICE`; root may raise to the real-time class.
#[test]
fn io_priorities_of_other_users_need_cap_sys_nice() {
    let (kernel, root, _) = machine_as(0, 0);
    let memory = memory();
    let caller = Caller::new(root, &memory);
    let child = fork_child(&kernel, &caller);
    let child_memory = memory_flat();
    let child_caller = Caller::new(child, &child_memory);
    let rt = ioprio(IoprioClass::IOPRIO_CLASS_RT, 2);
    let process = IoprioWho::IOPRIO_WHO_PROCESS.0;
    assert_eq!(block_on(kernel.ioprio_set(&caller, process, 0, rt)), Ok(0));
    assert_eq!(
        block_on(kernel.setresuid(&child_caller, 1000, 1000, 1000)),
        Ok(0)
    );
    let be = ioprio(IoprioClass::IOPRIO_CLASS_BE, 0);
    let theirs = kernel.ioprio_set(&child_caller, process, root.0 as i32, be);
    assert_eq!(errno(block_on(theirs)), Errno::EPERM);
    assert_eq!(
        block_on(kernel.ioprio_set(&caller, process, child.0 as i32, be)),
        Ok(0)
    );
}

/// `mseal` checks its arguments and that the range is mapped; a 32-bit kernel seals nothing.
#[test]
fn mseal_checks_and_seals_nothing_on_32_bit() {
    let (kernel, task, _) = setup(b"");
    let memory = memory_flat();
    let caller = Caller::new(task, &memory);
    let page = crate::mm::PAGE_SIZE as u32;
    let rw = ProtFlags::PROT_READ | ProtFlags::PROT_WRITE;
    let anon = MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS;
    let addr = block_on(kernel.mmap(&caller, 0, page, rw, anon, Fd(-1), 0)).unwrap();

    assert_eq!(block_on(kernel.mseal(&caller, addr, page, 0)), Ok(0));
    assert_eq!(
        errno(block_on(kernel.mseal(&caller, addr, page, 1))),
        Errno::EINVAL
    );
    assert_eq!(
        errno(block_on(kernel.mseal(&caller, addr + 1, page, 0))),
        Errno::EINVAL
    );
    assert_eq!(
        errno(block_on(kernel.mseal(&caller, addr, u32::MAX, 0))),
        Errno::EINVAL
    );
    assert_eq!(block_on(kernel.mseal(&caller, addr, 0, 0)), Ok(0));
    let past = kernel.mseal(&caller, addr + page, page, 0);
    assert_eq!(errno(block_on(past)), Errno::ENOMEM);
    // Nothing is sealed: the mapping still goes.
    assert_eq!(block_on(kernel.munmap(&caller, addr, page)), Ok(0));
}

/// `cachestat`: every page of the file's data in the range is cached and clean.
#[test]
fn cachestat_counts_the_files_pages() {
    let (kernel, task, _) = setup(b"");
    let memory = memory_flat();
    let caller = Caller::new(task, &memory);
    let page = crate::mm::PAGE_SIZE;
    let fd = super::mapping::open(
        &kernel,
        &caller,
        b"/f",
        OpenFlags::O_CREAT | OpenFlags::O_RDWR,
    );
    assert_eq!(
        block_on(kernel.ftruncate64(&caller, fd, (2 * page + 1) as i64)),
        Ok(0)
    );

    let stat = |off: u64, len: u64, flags: u32| {
        let range = CachestatRange { off, len };
        block_on(write_value(&memory, GuestPtrMut::new(0x100), &range)).unwrap();
        block_on(kernel.cachestat(
            &caller,
            fd,
            GuestPtr::new(0x100),
            GuestPtrMut::new(0x200),
            flags,
        ))?;
        let out: Cachestat = block_on(read_value(&memory, GuestPtr::new(0x200))).unwrap();
        Ok::<_, SysErr>(out)
    };
    assert_eq!(stat(0, 0, 0).unwrap().nr_cache, 3);
    assert_eq!(stat(page, page, 0).unwrap().nr_cache, 1);
    assert_eq!(stat(5 * page, 0, 0).unwrap().nr_cache, 0);
    let all = stat(0, 0, 0).unwrap();
    assert_eq!((all.nr_dirty, all.nr_writeback, all.nr_evicted), (0, 0, 0));
    assert_eq!(errno(stat(0, 0, 1)), Errno::EINVAL);
    assert_eq!(
        errno(block_on(kernel.cachestat(
            &caller,
            Fd(55),
            GuestPtr::new(0x100),
            GuestPtrMut::new(0x200),
            0
        ))),
        Errno::EBADF
    );
}

/// `process_madvise`: any advice on the caller's own memory (`PIDFD_SELF`), only the
/// non-destructive hints on another process's, with `CAP_SYS_NICE`.
#[test]
fn process_madvise_takes_hints_for_other_processes() {
    let (kernel, root, _) = machine_as(0, 0);
    let memory = memory();
    let caller = Caller::new(root, &memory);
    let page = crate::mm::PAGE_SIZE as u32;
    let rw = ProtFlags::PROT_READ | ProtFlags::PROT_WRITE;
    let anon = MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS;
    let addr = block_on(kernel.mmap(&caller, 0, page, rw, anon, Fd(-1), 0)).unwrap();
    put(&memory.0, 0x100, &addr.to_le_bytes());
    put(&memory.0, 0x104, &page.to_le_bytes());
    let vec = GuestPtr::new(0x100);
    let advise = |caller: &Caller<'_>, pidfd: Fd, advice: MadviseAdvice| {
        block_on(kernel.process_madvise(caller, pidfd, vec, 1, advice, 0))
    };
    let self_group = Fd(-10001);
    assert_eq!(
        advise(&caller, self_group, MadviseAdvice::MADV_DONTNEED),
        Ok(page as i32)
    );

    let child = fork_child(&kernel, &caller);
    let pidfd = block_on(kernel.pidfd_open(&caller, child.0 as i32, 0)).unwrap();
    assert_eq!(
        errno(advise(&caller, pidfd, MadviseAdvice::MADV_DONTNEED)),
        Errno::EINVAL
    );
    assert_eq!(
        advise(&caller, pidfd, MadviseAdvice::MADV_WILLNEED),
        Ok(page as i32)
    );
    assert_eq!(
        errno(block_on(kernel.process_madvise(
            &caller,
            pidfd,
            vec,
            1,
            MadviseAdvice::MADV_COLD,
            1
        ))),
        Errno::EINVAL
    );

    // Without CAP_SYS_NICE (the parent keeps every other capability).
    set_caps(&kernel, &caller, &memory.0, !(1 << 23) & ((1 << 41) - 1));
    assert_eq!(
        errno(advise(&caller, pidfd, MadviseAdvice::MADV_WILLNEED)),
        Errno::EPERM
    );
}

/// `vhangup` needs `CAP_SYS_TTY_CONFIG`; without a controlling terminal it has nothing to do.
#[test]
fn vhangup_needs_cap_sys_tty_config() {
    let (kernel, task, _) = setup(b"");
    let memory = memory_flat();
    let caller = Caller::new(task, &memory);
    assert_eq!(errno(block_on(kernel.vhangup(&caller))), Errno::EPERM);
    let (kernel, root, _) = machine_as(0, 0);
    let caller = Caller::new(root, &memory);
    assert_eq!(block_on(kernel.vhangup(&caller)), Ok(0));
}

/// Memory other tasks may reach from any thread, as the execution layer hands over a shared
/// memory ([`SharedMappings::attach_memory`]): `len` bytes, the first `guard` of them not
/// writable.
pub(super) struct SyncMemory {
    bytes: Mutex<Vec<u8>>,
    guard: u32,
}

impl SyncMemory {
    pub(super) fn new(len: usize, guard: u32) -> Arc<Self> {
        Arc::new(Self {
            bytes: Mutex::new(vec![0; len]),
            guard,
        })
    }

    pub(super) fn put(&self, addr: u32, data: &[u8]) {
        self.bytes.lock().unwrap()[addr as usize..][..data.len()].copy_from_slice(data);
    }

    pub(super) fn get(&self, addr: u32, len: usize) -> Vec<u8> {
        self.bytes.lock().unwrap()[addr as usize..][..len].to_vec()
    }
}

impl UserMemory for SyncMemory {
    fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        let bytes = self.bytes.lock().unwrap();
        let src = bytes.get(addr as usize..addr as usize + buf.len());
        let result = src.map(|src| buf.copy_from_slice(src)).ok_or(Fault);
        Box::pin(async move { result })
    }

    fn write<'a>(&'a self, addr: u32, data: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        let mut bytes = self.bytes.lock().unwrap();
        let result = match addr < self.guard {
            true => Err(Fault),
            false => bytes
                .get_mut(addr as usize..addr as usize + data.len())
                .map(|dst| dst.copy_from_slice(data))
                .ok_or(Fault),
        };
        Box::pin(async move { result })
    }

    fn size(&self) -> LocalFuture<'_, u64> {
        let size = self.bytes.lock().unwrap().len() as u64;
        Box::pin(async move { size })
    }

    fn grow(&self, _: u32) -> LocalFuture<'_, Option<u64>> {
        Box::pin(async { None })
    }
}

/// `access_remote_vm` copies through the memory the execution layer handed over, a page at a
/// time up to the first fault, and nothing once the last task left the memory.
#[test]
fn remote_access_goes_through_the_attached_memory_until_mmput() {
    let (kernel, task, _) = setup(b"");
    let memory = memory_flat();
    let caller = Caller::new(task, &memory);
    let page = crate::mm::PAGE_SIZE as u32;
    let mm = kernel.mm(task);
    let mut buf = [0; 4];
    let read = |mm: &crate::mm::Mm, addr: u32, buf: &mut [u8]| {
        block_on(crate::ptrace::access_remote_vm(
            mm,
            addr,
            crate::ptrace::Access::Read(buf),
            true,
        ))
    };
    assert_eq!(read(&mm, 0, &mut buf), 0, "nothing attached");

    let shared = SyncMemory::new(2 * page as usize, 16);
    shared.put(page - 2, b"abcd");
    kernel.attach_memory(task, shared.clone());
    assert_eq!(read(&mm, page - 2, &mut buf), 4);
    assert_eq!(&buf, b"abcd");
    let mut long = vec![0; 8];
    assert_eq!(read(&mm, 2 * page - 4, &mut long), 4, "up to the end");
    let write = crate::ptrace::Access::Write(b"wxyz");
    assert_eq!(
        block_on(crate::ptrace::access_remote_vm(&mm, 0x100, write, true)),
        4
    );
    assert_eq!(shared.get(0x100, 4), b"wxyz");
    let guard = crate::ptrace::Access::Write(b"z");
    assert_eq!(
        block_on(crate::ptrace::access_remote_vm(&mm, 0, guard, true)),
        0
    );

    block_on(kernel.exit_mmap(&caller));
    assert_eq!(read(&mm, page - 2, &mut buf), 0, "the last user left");
}
