//! Signal-driven I/O: an open file's owner (`F_SETOWN` and kin, the socket `ioctl`s), `O_ASYNC`
//! through `F_SETFL`, `FIOASYNC` and `open`, and the signals pipes, FIFOs, terminals, Unix
//! sockets and inotify send their files' owners (`fs/fcntl.c`).

use wasixv2_abi::{
    AddressFamily, CloneFlags, FOwnerEx, FOwnerType, FcntlCmd, InotifyInitFlags, InotifyMask,
    IoctlRequest, KernelTimespec, Mode, Siginfo, Signal, SigprocmaskHow, Sigset,
};
use wasixv2_syscalls::{read_value, write_value};
use wasixv2_vfs::MountFlags;

use super::{
    ipc::{errno, pipe, put},
    proc::ForkableMemory,
    *,
};
use crate::fs::FASYNC;

const CWD: Fd = Fd(-100);
const SIGIO: Signal = Signal::SIGIO;
const SIGUSR1: Signal = Signal::SIGUSR1;
/// `SIGRTMIN`, which queues each signal with its `siginfo`.
const SIGRTMIN: Signal = Signal(34);
const POLLIN_BAND: i32 = 0x41;
const POLLOUT_BAND: i32 = 0x304;
const POLLHUP_BAND: i32 = 0x18;

/// A process of the test: its first task and its memory.
struct P {
    task: TaskId,
    memory: ForkableMemory,
}

impl P {
    fn new(task: TaskId) -> Self {
        Self {
            task,
            memory: ForkableMemory(FlatMemory(RefCell::new(vec![0; 65536]))),
        }
    }

    fn caller(&self) -> Caller<'_> {
        Caller::new(self.task, &self.memory)
    }
}

/// A kernel with its init, `/dev` and `/dev/pts`, and one process, run as root.
fn machine() -> (Kernel, P) {
    let (kernel, task, _) = super::cred::machine_with(|kernel| {
        kernel.with_credentials(Credentials::new(0, 0)).with_init()
    });
    let vfs = kernel.vfs().clone();
    let clock = Arc::new(HostClock(Arc::new(TestHost)));
    let dev = MemFs::new(clock, 0o755, 0, 0);
    block_on(vfs.mount(b"/dev", Arc::new(dev), MountFlags::default(), true)).unwrap();
    block_on(kernel.create_device_nodes(b"/dev")).unwrap();
    block_on(vfs.mount(b"/dev/pts", kernel.devpts(), MountFlags::default(), true)).unwrap();
    (kernel, P::new(task))
}

/// `clone` with `flags` and SIGCHLD: the child, and the pid clone returned.
fn fork(kernel: &Kernel, parent: &P, flags: u32) -> (P, Pid) {
    let null = GuestPtrMut::NULL;
    let flags = flags | Signal::SIGCHLD.0 as u32;
    let pid = block_on(kernel.clone(&parent.caller(), flags, 0, null, 0, null)).unwrap();
    let mut child = None;
    while let Some(action) = kernel.take_pending(parent.task) {
        if let GuestAction::Fork { child: task, .. } = action {
            child = Some(task);
        }
    }
    (P::new(child.expect("fork left the child")), pid)
}

fn fcntl(kernel: &Kernel, p: &P, fd: Fd, cmd: FcntlCmd, arg: u32) -> SysResult<i32> {
    block_on(kernel.fcntl64(&p.caller(), fd, cmd, arg))
}

fn getpid(kernel: &Kernel, p: &P) -> Pid {
    block_on(kernel.getpid(&p.caller())).unwrap()
}

/// `F_SETFL` with `O_ASYNC` on (`on`) or off, the other status flags kept.
fn set_async(kernel: &Kernel, p: &P, fd: Fd, on: bool) {
    let flags = fcntl(kernel, p, fd, FcntlCmd::F_GETFL, 0).unwrap() as u32;
    let flags = match on {
        true => flags | FASYNC,
        false => flags & !FASYNC,
    };
    assert_eq!(fcntl(kernel, p, fd, FcntlCmd::F_SETFL, flags), Ok(0));
}

fn has_async(kernel: &Kernel, p: &P, fd: Fd) -> bool {
    fcntl(kernel, p, fd, FcntlCmd::F_GETFL, 0).unwrap() as u32 & FASYNC != 0
}

/// `ioctl(fd, FIOASYNC, &on)`.
fn fioasync(kernel: &Kernel, p: &P, fd: Fd, on: bool) -> SysResult<i32> {
    put(&p.memory.0, 0x60, &i32::from(on).to_le_bytes());
    block_on(kernel.ioctl(&p.caller(), fd, IoctlRequest::FIOASYNC, 0x60))
}

/// `F_GETOWN_EX`: the owner's kind and number.
fn getown_ex(kernel: &Kernel, p: &P, fd: Fd) -> (FOwnerType, Pid) {
    assert_eq!(fcntl(kernel, p, fd, FcntlCmd::F_GETOWN_EX, 0x900), Ok(0));
    let owner: FOwnerEx = block_on(read_value(&p.memory, GuestPtr::new(0x900))).unwrap();
    (owner.r#type, owner.pid)
}

fn setown_ex(kernel: &Kernel, p: &P, fd: Fd, ty: FOwnerType, pid: Pid) -> SysResult<i32> {
    let owner = FOwnerEx { r#type: ty, pid };
    block_on(write_value(&p.memory, GuestPtrMut::new(0x900), &owner)).unwrap();
    fcntl(kernel, p, fd, FcntlCmd::F_SETOWN_EX, 0x900)
}

fn block(kernel: &Kernel, p: &P, sigs: &[Signal]) {
    let mask: u64 = sigs.iter().map(|sig| 1u64 << (sig.0 - 1)).sum();
    let set = Sigset {
        sig: [mask as u32, (mask >> 32) as u32],
    };
    block_on(write_value(&p.memory, GuestPtrMut::new(0x160), &set)).unwrap();
    let how = SigprocmaskHow::SIG_BLOCK;
    let caller = p.caller();
    let set = kernel.rt_sigprocmask(&caller, how, GuestPtr::new(0x160), GuestPtrMut::NULL, 8);
    assert_eq!(block_on(set), Ok(0));
}

/// `sigtimedwait` for `sig` without waiting: its `siginfo`, if it is pending.
fn take(kernel: &Kernel, p: &P, sig: Signal) -> Option<Siginfo> {
    let mask = 1u64 << (sig.0 - 1);
    let set = Sigset {
        sig: [mask as u32, (mask >> 32) as u32],
    };
    let now = KernelTimespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    block_on(write_value(&p.memory, GuestPtrMut::new(0x170), &set)).unwrap();
    block_on(write_value(&p.memory, GuestPtrMut::new(0x178), &now)).unwrap();
    let caller = p.caller();
    let wait = kernel.rt_sigtimedwait_time64(
        &caller,
        GuestPtr::new(0x170),
        GuestPtrMut::new(0x200),
        GuestPtr::new(0x178),
        8,
    );
    match block_on(wait) {
        Ok(got) => {
            assert_eq!(got, sig.0);
            Some(block_on(read_value(&p.memory, GuestPtr::new(0x200))).unwrap())
        }
        Err(SysErr::Errno(Errno::EAGAIN)) => None,
        Err(other) => panic!("sigtimedwait failed: {other:?}"),
    }
}

/// `si_code`, `si_band` and `si_fd` of an I/O signal.
fn poll_info(info: &Siginfo) -> (i32, i32, i32) {
    let field = |at: usize| i32::from_le_bytes(info.sifields[at..at + 4].try_into().unwrap());
    (info.si_code, field(0), field(4))
}

fn write(kernel: &Kernel, p: &P, fd: Fd, data: &[u8]) -> SysResult<u32> {
    put(&p.memory.0, 0x400, data);
    let len = data.len() as u32;
    block_on(kernel.write(&p.caller(), fd, UserSlice::new(0x400, len)))
}

fn read(kernel: &Kernel, p: &P, fd: Fd, len: u32) -> SysResult<u32> {
    block_on(kernel.read(&p.caller(), fd, UserSliceMut::new(0x400, len)))
}

fn close(kernel: &Kernel, p: &P, fd: Fd) {
    assert_eq!(block_on(kernel.close(&p.caller(), fd)), Ok(0));
}

fn open(kernel: &Kernel, p: &P, path: &str, flags: OpenFlags) -> SysResult<Fd> {
    let mut name = path.as_bytes().to_vec();
    name.push(0);
    put(&p.memory.0, 0x800, &name);
    block_on(kernel.openat(&p.caller(), CWD, GuestPtr::new(0x800), flags, 0))
}

// ---- owners ----

/// `F_SETOWN` takes a process, or a negated process group, by its number in the caller's pid
/// namespace; `F_GETOWN` and `F_GETOWN_EX` give it back with its kind, `F_SETOWN_EX` takes a
/// thread too. A file that never had an owner reports a thread owner 0.
#[test]
fn an_owner_is_a_thread_a_process_or_a_group() {
    let (kernel, p) = machine();
    let (r, _w) = pipe(&kernel, &p.caller(), OpenFlags(0));
    let pid = getpid(&kernel, &p);
    let pgid = block_on(kernel.getpgid(&p.caller(), 0)).unwrap();
    let get = |fd| fcntl(&kernel, &p, fd, FcntlCmd::F_GETOWN, 0);

    assert_eq!(get(r), Ok(0));
    assert_eq!(getown_ex(&kernel, &p, r), (FOwnerType::F_OWNER_TID, 0));

    assert_eq!(fcntl(&kernel, &p, r, FcntlCmd::F_SETOWN, pid as u32), Ok(0));
    assert_eq!(get(r), Ok(pid));
    assert_eq!(getown_ex(&kernel, &p, r), (FOwnerType::F_OWNER_PID, pid));

    let minus = (-pgid) as u32;
    assert_eq!(fcntl(&kernel, &p, r, FcntlCmd::F_SETOWN, minus), Ok(0));
    assert_eq!(get(r), Ok(-pgid));
    assert_eq!(getown_ex(&kernel, &p, r), (FOwnerType::F_OWNER_PGRP, pgid));

    let tid = block_on(kernel.gettid(&p.caller())).unwrap();
    assert_eq!(
        setown_ex(&kernel, &p, r, FOwnerType::F_OWNER_TID, tid),
        Ok(0)
    );
    assert_eq!(getown_ex(&kernel, &p, r), (FOwnerType::F_OWNER_TID, tid));
    let bad = setown_ex(&kernel, &p, r, FOwnerType(3), pid);
    assert_eq!(errno(bad), Errno::EINVAL);
    let gone = setown_ex(&kernel, &p, r, FOwnerType::F_OWNER_PID, 4000);
    assert_eq!(errno(gone), Errno::ESRCH);

    let set = |who: i32| fcntl(&kernel, &p, r, FcntlCmd::F_SETOWN, who as u32);
    assert_eq!(errno(set(4000)), Errno::ESRCH);
    assert_eq!(errno(set(i32::MIN)), Errno::EINVAL);
    // `f_setown(filp, 0, 1)` clears the owner and leaves a process kind.
    assert_eq!(set(0), Ok(0));
    assert_eq!(get(r), Ok(0));
    assert_eq!(getown_ex(&kernel, &p, r), (FOwnerType::F_OWNER_PID, 0));

    // Who set the owner: root.
    assert_eq!(set(pid), Ok(0));
    assert_eq!(
        fcntl(&kernel, &p, r, FcntlCmd::F_GETOWNER_UIDS, 0x940),
        Ok(0)
    );
    let uids: [u32; 2] = block_on(read_value(&p.memory, GuestPtr::new(0x940))).unwrap();
    assert_eq!(uids, [0, 0]);

    // An `O_PATH` descriptor has none of this.
    let path = open(&kernel, &p, "/", OpenFlags::O_PATH).unwrap();
    assert_eq!(
        errno(fcntl(&kernel, &p, path, FcntlCmd::F_SETOWN, pid as u32)),
        Errno::EBADF
    );
}

/// `F_SETSIG` takes 0 (plain SIGIO) or any signal up to 64.
#[test]
fn f_setsig_takes_any_signal_or_zero() {
    let (kernel, p) = machine();
    let (r, _w) = pipe(&kernel, &p.caller(), OpenFlags(0));
    let sig = |cmd, arg| fcntl(&kernel, &p, r, cmd, arg);

    assert_eq!(sig(FcntlCmd::F_GETSIG, 0), Ok(0));
    assert_eq!(sig(FcntlCmd::F_SETSIG, SIGUSR1.0 as u32), Ok(0));
    assert_eq!(sig(FcntlCmd::F_GETSIG, 0), Ok(SIGUSR1.0));
    assert_eq!(sig(FcntlCmd::F_SETSIG, 64), Ok(0));
    assert_eq!(errno(sig(FcntlCmd::F_SETSIG, 65)), Errno::EINVAL);
    assert_eq!(errno(sig(FcntlCmd::F_SETSIG, u32::MAX)), Errno::EINVAL);
    assert_eq!(sig(FcntlCmd::F_GETSIG, 0), Ok(64));
}

/// The owner is kept as the process, not its number: inside a pid namespace it reads as the
/// number it has there, outside as its own there, and once it is gone as 0.
#[test]
fn an_owner_reads_as_each_pid_namespace_numbers_it_and_as_zero_once_gone() {
    let (kernel, p) = machine();
    let (r, _w) = pipe(&kernel, &p.caller(), OpenFlags(0));
    let (child, pid) = fork(&kernel, &p, CloneFlags::CLONE_NEWPID.0 as u32);
    assert_eq!(fcntl(&kernel, &child, r, FcntlCmd::F_SETOWN, 1), Ok(0));

    assert_eq!(fcntl(&kernel, &child, r, FcntlCmd::F_GETOWN, 0), Ok(1));
    assert_eq!(fcntl(&kernel, &p, r, FcntlCmd::F_GETOWN, 0), Ok(pid));

    block_on(kernel.exit_group(&child.caller(), 0)).unwrap();
    kernel.release(child.task);
    assert_eq!(
        fcntl(&kernel, &p, r, FcntlCmd::F_GETOWN, 0),
        Ok(pid),
        "a zombie"
    );
    let options = wasixv2_abi::WaitOptions::WEXITED;
    let info = wasixv2_abi::SiginfoPtr::new(0x300);
    let null = wasixv2_abi::RusagePtr::NULL;
    let caller = p.caller();
    let reaped = kernel.waitid(
        &caller,
        wasixv2_abi::Idtype::P_PID,
        pid,
        info,
        options,
        null,
    );
    assert_eq!(block_on(reaped), Ok(0));
    assert_eq!(fcntl(&kernel, &p, r, FcntlCmd::F_GETOWN, 0), Ok(0));
    assert_eq!(getown_ex(&kernel, &p, r), (FOwnerType::F_OWNER_PID, 0));
}

// ---- pipes ----

/// `O_ASYNC` on a pipe's read end has its owner hear of data, with plain SIGIO or with the
/// `F_SETSIG` signal saying `POLL_IN`, the band and the descriptor; on the write end, of room
/// a read made. Taking `O_ASYNC` off ends it.
#[test]
fn a_pipe_signals_its_readers_of_data_and_its_writers_of_room() {
    let (kernel, p) = machine();
    let (r, w) = pipe(&kernel, &p.caller(), OpenFlags::O_NONBLOCK);
    let pid = getpid(&kernel, &p) as u32;
    block(&kernel, &p, &[SIGIO, SIGUSR1, SIGRTMIN]);
    assert_eq!(fcntl(&kernel, &p, r, FcntlCmd::F_SETOWN, pid), Ok(0));

    // Without `O_ASYNC`, nothing.
    assert_eq!(write(&kernel, &p, w, b"a"), Ok(1));
    assert!(take(&kernel, &p, SIGIO).is_none());

    set_async(&kernel, &p, r, true);
    assert!(has_async(&kernel, &p, r));
    assert_eq!(write(&kernel, &p, w, b"b"), Ok(1));
    let info = take(&kernel, &p, SIGIO).expect("SIGIO");
    assert_eq!(info.si_code, wasixv2_abi::SiCode::SI_KERNEL.0);

    let usr1 = SIGUSR1.0 as u32;
    assert_eq!(fcntl(&kernel, &p, r, FcntlCmd::F_SETSIG, usr1), Ok(0));
    assert_eq!(write(&kernel, &p, w, b"c"), Ok(1));
    let info = take(&kernel, &p, SIGUSR1).expect("the F_SETSIG signal");
    assert_eq!(poll_info(&info), (1, POLLIN_BAND, r.0), "POLL_IN");
    assert!(take(&kernel, &p, SIGIO).is_none());

    // The write end hears of room once its owner asked.
    assert_eq!(fcntl(&kernel, &p, w, FcntlCmd::F_SETOWN, pid), Ok(0));
    let rtmin = SIGRTMIN.0 as u32;
    assert_eq!(fcntl(&kernel, &p, w, FcntlCmd::F_SETSIG, rtmin), Ok(0));
    assert_eq!(fioasync(&kernel, &p, w, true), Ok(0));
    assert!(has_async(&kernel, &p, w));
    assert_eq!(read(&kernel, &p, r, 8), Ok(3));
    let info = take(&kernel, &p, SIGRTMIN).expect("the writer's signal");
    assert_eq!(poll_info(&info), (2, POLLOUT_BAND, w.0), "POLL_OUT");
    // An empty pipe's non-blocking read signals the writers too (`pipe_read`).
    assert_eq!(errno(read(&kernel, &p, r, 8)), Errno::EAGAIN);
    assert!(take(&kernel, &p, SIGRTMIN).is_some());

    set_async(&kernel, &p, r, false);
    assert!(!has_async(&kernel, &p, r));
    assert_eq!(write(&kernel, &p, w, b"d"), Ok(1));
    assert!(take(&kernel, &p, SIGUSR1).is_none());
}

/// The last writer's close tells the readers (`pipe_release`), and a closed file is off the
/// lists: closing the read end signals nobody.
#[test]
fn the_last_end_of_one_side_closing_signals_the_other_side() {
    let (kernel, p) = machine();
    let (r, w) = pipe(&kernel, &p.caller(), OpenFlags::O_NONBLOCK);
    let pid = getpid(&kernel, &p) as u32;
    block(&kernel, &p, &[SIGUSR1]);
    let usr1 = SIGUSR1.0 as u32;
    for fd in [r, w] {
        assert_eq!(fcntl(&kernel, &p, fd, FcntlCmd::F_SETOWN, pid), Ok(0));
        assert_eq!(fcntl(&kernel, &p, fd, FcntlCmd::F_SETSIG, usr1), Ok(0));
    }
    set_async(&kernel, &p, r, true);
    let copy = fcntl(&kernel, &p, w, FcntlCmd::F_DUPFD, 0).map(Fd).unwrap();

    close(&kernel, &p, w);
    assert!(take(&kernel, &p, SIGUSR1).is_none(), "a writer is left");
    close(&kernel, &p, copy);
    let info = take(&kernel, &p, SIGUSR1).expect("the readers hear of the end");
    assert_eq!(poll_info(&info), (1, POLLIN_BAND, r.0));

    close(&kernel, &p, r);
    assert!(take(&kernel, &p, SIGUSR1).is_none());
}

/// `O_ASYNC` given to `open` is kept in the flags but puts the file on no list, and then
/// sticks, as on Linux: `F_SETFL` and `FIOASYNC` see no change to make with it on, and taking it
/// off finds the file on no list, which leaves the flag (`fasync_remove_entry`).
#[test]
fn o_async_from_open_sticks_and_signals_nothing() {
    let (kernel, p) = machine();
    put(&p.memory.0, 0x800, b"/fifo\0");
    let mode: Mode = 0o010600;
    let made = block_on(kernel.mknodat(&p.caller(), CWD, GuestPtr::new(0x800), mode, 0));
    assert_eq!(made, Ok(0));
    let flags = OpenFlags::O_RDWR | OpenFlags::O_NONBLOCK | OpenFlags(FASYNC);
    let fd = open(&kernel, &p, "/fifo", flags).unwrap();
    let pid = getpid(&kernel, &p) as u32;
    assert_eq!(fcntl(&kernel, &p, fd, FcntlCmd::F_SETOWN, pid), Ok(0));
    block(&kernel, &p, &[SIGIO]);

    assert!(has_async(&kernel, &p, fd));
    assert_eq!(write(&kernel, &p, fd, b"a"), Ok(1));
    assert!(take(&kernel, &p, SIGIO).is_none());
    set_async(&kernel, &p, fd, true);
    assert_eq!(write(&kernel, &p, fd, b"b"), Ok(1));
    assert!(take(&kernel, &p, SIGIO).is_none());

    assert_eq!(fioasync(&kernel, &p, fd, true), Ok(0));
    assert_eq!(write(&kernel, &p, fd, b"c"), Ok(1));
    assert!(take(&kernel, &p, SIGIO).is_none());

    assert_eq!(fioasync(&kernel, &p, fd, false), Ok(0));
    set_async(&kernel, &p, fd, false);
    assert!(has_async(&kernel, &p, fd), "on no list, so it stays");

    // A file opened without it works as any other.
    let flags = OpenFlags::O_RDWR | OpenFlags::O_NONBLOCK;
    let other = open(&kernel, &p, "/fifo", flags).unwrap();
    assert_eq!(fcntl(&kernel, &p, other, FcntlCmd::F_SETOWN, pid), Ok(0));
    assert_eq!(fioasync(&kernel, &p, other, true), Ok(0));
    assert_eq!(write(&kernel, &p, fd, b"d"), Ok(1));
    assert!(take(&kernel, &p, SIGIO).is_some());
}

/// A group owner's signal goes to every process of the group, a thread owner's to that thread
/// alone.
#[test]
fn a_group_owner_signals_each_process_and_a_thread_owner_its_thread() {
    let (kernel, p) = machine();
    let (r, w) = pipe(&kernel, &p.caller(), OpenFlags::O_NONBLOCK);
    block(&kernel, &p, &[SIGIO]);
    let (child, _) = fork(&kernel, &p, 0);
    let pgid = block_on(kernel.getpgid(&p.caller(), 0)).unwrap();
    assert_eq!(
        fcntl(&kernel, &p, r, FcntlCmd::F_SETOWN, (-pgid) as u32),
        Ok(0)
    );
    set_async(&kernel, &p, r, true);

    assert_eq!(write(&kernel, &p, w, b"a"), Ok(1));
    assert!(take(&kernel, &p, SIGIO).is_some(), "the parent");
    assert!(take(&kernel, &child, SIGIO).is_some(), "the child");

    let null = GuestPtrMut::NULL;
    let tid = block_on(kernel.thread_spawn(&p.caller(), 0x10f00, 0, null, 0, null)).unwrap();
    while kernel.take_pending(p.task).is_some() {}
    // The thread's own view of the memory is all `take` needs.
    let thread = P::new(TaskId(tid as u32));
    assert_eq!(
        setown_ex(&kernel, &p, r, FOwnerType::F_OWNER_TID, tid),
        Ok(0)
    );
    assert_eq!(write(&kernel, &p, w, b"b"), Ok(1));
    assert!(take(&kernel, &p, SIGIO).is_none(), "not the leader");
    assert!(take(&kernel, &child, SIGIO).is_none());
    assert!(take(&kernel, &thread, SIGIO).is_some(), "the thread");
}

/// `sigio_perm`: the signal reaches a receiver only if whoever set the owner is root or shares
/// a uid with the receiver's real or saved one.
#[test]
fn only_a_receiver_whose_uids_match_the_setters_gets_the_signal() {
    let (kernel, p) = machine();
    let (r, w) = pipe(&kernel, &p.caller(), OpenFlags::O_NONBLOCK);
    let (child, pid) = fork(&kernel, &p, 0);
    block(&kernel, &child, &[SIGIO]);
    let to = |p: &P, uid: u32| block_on(kernel.setresuid(&p.caller(), uid, uid, uid));
    assert_eq!(to(&child, 2000), Ok(0));
    set_async(&kernel, &p, r, true);

    assert_eq!(fcntl(&kernel, &p, r, FcntlCmd::F_SETOWN, pid as u32), Ok(0));
    assert_eq!(write(&kernel, &p, w, b"a"), Ok(1));
    assert!(take(&kernel, &child, SIGIO).is_some(), "set by root");

    assert_eq!(to(&p, 1000), Ok(0));
    assert_eq!(fcntl(&kernel, &p, r, FcntlCmd::F_SETOWN, pid as u32), Ok(0));
    assert_eq!(write(&kernel, &p, w, b"b"), Ok(1));
    assert!(take(&kernel, &child, SIGIO).is_none(), "set by uid 1000");
}

// ---- terminals, sockets, inotify ----

/// A terminal puts the file's owner in place when `O_ASYNC` goes on, if it has none: the
/// foreground group, or else the caller. Its input then signals `POLL_IN`.
#[test]
fn a_pty_master_signals_its_input_to_the_caller_it_made_owner() {
    let (kernel, p) = machine();
    let rw = OpenFlags::O_RDWR | OpenFlags::O_NOCTTY | OpenFlags::O_NONBLOCK;
    let master = open(&kernel, &p, "/dev/ptmx", rw).unwrap();
    put(&p.memory.0, 0x60, &0i32.to_le_bytes());
    let ioctl = |request| block_on(kernel.ioctl(&p.caller(), master, request, 0x60));
    assert_eq!(ioctl(IoctlRequest::TIOCSPTLCK), Ok(0));
    assert_eq!(ioctl(IoctlRequest::TIOCGPTN), Ok(0));
    let n: i32 = block_on(read_value(&p.memory, GuestPtr::new(0x60))).unwrap();
    let slave = open(&kernel, &p, &format!("/dev/pts/{n}"), rw).unwrap();
    block(&kernel, &p, &[SIGUSR1]);

    assert_eq!(fioasync(&kernel, &p, master, true), Ok(0));
    let pid = getpid(&kernel, &p);
    assert_eq!(fcntl(&kernel, &p, master, FcntlCmd::F_GETOWN, 0), Ok(pid));
    let usr1 = SIGUSR1.0 as u32;
    assert_eq!(fcntl(&kernel, &p, master, FcntlCmd::F_SETSIG, usr1), Ok(0));
    assert_eq!(write(&kernel, &p, slave, b"x"), Ok(1));
    let info = take(&kernel, &p, SIGUSR1).expect("the master's input");
    assert_eq!(poll_info(&info), (1, POLLIN_BAND, master.0));
}

/// Sockets take their owner through `FIOSETOWN` and `SIOCSPGRP` too; a Unix socket signals
/// its data and its peer's close. Other files do not know these requests.
#[test]
fn a_unix_socket_signals_data_and_its_peers_close() {
    let (kernel, p) = machine();
    let pair = wasixv2_abi::FdPair::new(0x100);
    let made = block_on(kernel.socketpair(&p.caller(), AddressFamily::AF_UNIX, 1, 0, pair));
    assert_eq!(made, Ok(0));
    let [a, b]: [i32; 2] = block_on(read_value(&p.memory, GuestPtr::new(0x100))).unwrap();
    let (a, b) = (Fd(a), Fd(b));
    let pid = getpid(&kernel, &p);
    let ioctl = |fd, request| block_on(kernel.ioctl(&p.caller(), fd, request, 0x60));
    let owner = || i32::from_le_bytes(super::ipc::get(&p.memory.0, 0x60, 4).try_into().unwrap());

    put(&p.memory.0, 0x60, &pid.to_le_bytes());
    assert_eq!(ioctl(a, IoctlRequest::FIOSETOWN), Ok(0));
    put(&p.memory.0, 0x60, &0i32.to_le_bytes());
    assert_eq!(ioctl(a, IoctlRequest::SIOCGPGRP), Ok(0));
    assert_eq!(owner(), pid);
    assert_eq!(ioctl(a, IoctlRequest::FIOGETOWN), Ok(0));
    assert_eq!(owner(), pid);
    let (r, _w) = pipe(&kernel, &p.caller(), OpenFlags(0));
    assert_eq!(errno(ioctl(r, IoctlRequest::FIOSETOWN)), Errno::ENOTTY);

    block(&kernel, &p, &[SIGUSR1]);
    let usr1 = SIGUSR1.0 as u32;
    assert_eq!(fcntl(&kernel, &p, a, FcntlCmd::F_SETSIG, usr1), Ok(0));
    set_async(&kernel, &p, a, true);
    assert_eq!(write(&kernel, &p, b, b"hi"), Ok(2));
    let info = take(&kernel, &p, SIGUSR1).expect("data");
    assert_eq!(poll_info(&info), (1, POLLIN_BAND, a.0), "POLL_IN");
    close(&kernel, &p, b);
    let info = take(&kernel, &p, SIGUSR1).expect("the peer's close");
    assert_eq!(poll_info(&info), (6, POLLHUP_BAND, a.0), "POLL_HUP");
}

/// An inotify instance signals each event it queues (`fsnotify_insert_event`).
#[test]
fn inotify_signals_its_events() {
    let (kernel, p) = machine();
    let fd = block_on(kernel.inotify_init1(&p.caller(), InotifyInitFlags::IN_NONBLOCK)).unwrap();
    put(&p.memory.0, 0x800, b"/\0");
    let mask = InotifyMask::IN_CREATE;
    let add = block_on(kernel.inotify_add_watch(&p.caller(), fd, GuestPtr::new(0x800), mask));
    assert!(add.is_ok());
    let pid = getpid(&kernel, &p) as u32;
    assert_eq!(fcntl(&kernel, &p, fd, FcntlCmd::F_SETOWN, pid), Ok(0));
    set_async(&kernel, &p, fd, true);
    block(&kernel, &p, &[SIGIO]);

    let created = open(
        &kernel,
        &p,
        "/new",
        OpenFlags::O_CREAT | OpenFlags::O_WRONLY,
    );
    assert!(created.is_ok());
    assert!(take(&kernel, &p, SIGIO).is_some());
}

/// An IP socket's readiness reaches the kernel only in its wakeups, so its signals go out from
/// the timer driver right after: a datagram arriving signals `POLL_IN`.
#[test]
fn a_udp_socket_signals_a_datagram_once_the_timer_driver_ran() {
    let host: Arc<dyn Host> = Arc::new(super::ptimer::RealHost(std::time::Instant::now()));
    let root = MemFs::new(Arc::new(HostClock(host.clone())), 0o755, 0, 0);
    let network = wasixv2_vnet::Net::new(wasixv2_vnet::NetConfig::default(), None);
    let kernel = Kernel::new(host, Arc::new(root)).with_network(network);
    let stdin = Arc::new(Input(Mutex::new(Vec::new())));
    let (kernel, task, _) = spawn_on(kernel, stdin, &[], &[]);
    let p = P::new(task);
    let socket = block_on(kernel.socket(&p.caller(), AddressFamily::AF_INET, 2, 0)).unwrap();
    // 127.0.0.1:4242.
    let addr = [2, 0, 0x10, 0x92, 127, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0];
    put(&p.memory.0, 0x500, &addr);
    let bound = block_on(kernel.bind(&p.caller(), socket, GuestPtr::new(0x500), 16));
    assert_eq!(bound, Ok(0));
    let pid = getpid(&kernel, &p) as u32;
    assert_eq!(fcntl(&kernel, &p, socket, FcntlCmd::F_SETOWN, pid), Ok(0));
    let usr1 = SIGUSR1.0 as u32;
    assert_eq!(fcntl(&kernel, &p, socket, FcntlCmd::F_SETSIG, usr1), Ok(0));
    set_async(&kernel, &p, socket, true);
    block(&kernel, &p, &[SIGUSR1]);

    put(&p.memory.0, 0x400, b"ping");
    let flags = wasixv2_abi::MsgFlags(0);
    let to = GuestPtr::new(0x500);
    let caller = p.caller();
    let sent = kernel.sendto(&caller, socket, UserSlice::new(0x400, 4), flags, to, 16);
    assert_eq!(block_on(sent), Ok(4));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let info = loop {
        if let Some(info) = take(&kernel, &p, SIGUSR1) {
            break info;
        }
        assert!(std::time::Instant::now() < deadline, "no signal");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(poll_info(&info), (1, POLLIN_BAND, socket.0), "POLL_IN");
}

// ---- urgent data ----

/// A machine on a network of its own, and a connected TCP pair in its one process: the
/// connecting socket and the accepted one, `port` on 127.0.0.1.
fn tcp_pair(port: u16) -> (Kernel, P, Fd, Fd) {
    let host: Arc<dyn Host> = Arc::new(super::ptimer::RealHost(std::time::Instant::now()));
    let root = MemFs::new(Arc::new(HostClock(host.clone())), 0o755, 0, 0);
    let network = wasixv2_vnet::Net::new(wasixv2_vnet::NetConfig::default(), None);
    let kernel = Kernel::new(host, Arc::new(root)).with_network(network);
    let stdin = Arc::new(Input(Mutex::new(Vec::new())));
    let (kernel, task, _) = spawn_on(kernel, stdin, &[], &[]);
    let p = P::new(task);
    let caller = p.caller();
    let stream =
        |kernel: &Kernel| block_on(kernel.socket(&caller, AddressFamily::AF_INET, 1, 0)).unwrap();
    let listener = stream(&kernel);
    let [hi, lo] = port.to_be_bytes();
    put(
        &p.memory.0,
        0x500,
        &[2, 0, hi, lo, 127, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0],
    );
    let addr = GuestPtr::new(0x500);
    assert_eq!(block_on(kernel.bind(&caller, listener, addr, 16)), Ok(0));
    assert_eq!(block_on(kernel.listen(&caller, listener, 1)), Ok(0));
    let client = stream(&kernel);
    assert_eq!(block_on(kernel.connect(&caller, client, addr, 16)), Ok(0));
    let (null, nolen) = (GuestPtrMut::NULL, GuestPtrMut::NULL);
    let flags = wasixv2_abi::SocketFlags::SOCK_NONBLOCK;
    let server = block_on(kernel.accept4(&caller, listener, null, nolen, flags)).unwrap();
    (kernel, p, client, server)
}

fn send(kernel: &Kernel, p: &P, fd: Fd, data: &[u8], flags: wasixv2_abi::MsgFlags) {
    put(&p.memory.0, 0x400, data);
    let len = data.len() as u32;
    let caller = p.caller();
    let sent = kernel.sendto(
        &caller,
        fd,
        UserSlice::new(0x400, len),
        flags,
        GuestPtr::NULL,
        0,
    );
    assert_eq!(block_on(sent), Ok(len));
}

fn recv(
    kernel: &Kernel,
    p: &P,
    fd: Fd,
    len: u32,
    flags: wasixv2_abi::MsgFlags,
) -> SysResult<Vec<u8>> {
    let (null, nolen) = (GuestPtrMut::NULL, GuestPtrMut::NULL);
    let caller = p.caller();
    let got = kernel.recvfrom(
        &caller,
        fd,
        UserSliceMut::new(0x600, len),
        flags,
        null,
        nolen,
    );
    let n = block_on(got)?;
    Ok(super::ipc::get(&p.memory.0, 0x600, n as usize))
}

/// Waits for `sig`, which the timer driver sends.
fn wait_for(kernel: &Kernel, p: &P, sig: Signal) -> Siginfo {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(info) = take(kernel, p, sig) {
            return info;
        }
        assert!(std::time::Instant::now() < deadline, "no {sig:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A new urgent mark sends `SIGURG` to the socket's owner, `O_ASYNC` or not (`sk_send_sigurg`),
/// and to files with an `F_SETSIG` signal on the socket's list that signal with `POLL_PRI` too,
/// before the data's `POLL_IN`. A socket without an owner signals nobody.
#[test]
fn urgent_data_signals_sigurg_to_the_socket_owner() {
    let oob = wasixv2_abi::MsgFlags::MSG_OOB;
    let (kernel, p, client, server) = tcp_pair(4300);
    block(&kernel, &p, &[Signal::SIGURG, SIGRTMIN, SIGIO]);
    send(&kernel, &p, client, b"!", oob);
    std::thread::sleep(Duration::from_millis(50));
    assert!(take(&kernel, &p, Signal::SIGURG).is_none(), "no owner");

    let pid = getpid(&kernel, &p) as u32;
    assert_eq!(fcntl(&kernel, &p, server, FcntlCmd::F_SETOWN, pid), Ok(0));
    send(&kernel, &p, client, b"xU", oob);
    let info = wait_for(&kernel, &p, Signal::SIGURG);
    assert_eq!(info.si_code, wasixv2_abi::SiCode::SI_KERNEL.0);
    assert!(take(&kernel, &p, SIGRTMIN).is_none(), "not on the list");

    let rt = SIGRTMIN.0 as u32;
    assert_eq!(fcntl(&kernel, &p, server, FcntlCmd::F_SETSIG, rt), Ok(0));
    set_async(&kernel, &p, server, true);
    send(&kernel, &p, client, b"V", oob);
    wait_for(&kernel, &p, Signal::SIGURG);
    let pri = wait_for(&kernel, &p, SIGRTMIN);
    assert_eq!(poll_info(&pri), (5, 0x82, server.0), "POLL_PRI");
    let data = wait_for(&kernel, &p, SIGRTMIN);
    assert_eq!(poll_info(&data), (1, POLLIN_BAND, server.0), "POLL_IN");
}

/// What the reader sees of urgent data: `POLLPRI` until `recv(MSG_OOB)` takes the byte, reads
/// that stop at the mark, `SIOCATMARK`, and `MSG_OOB` that never blocks.
#[test]
fn urgent_data_reaches_recv_ioctl_and_poll() {
    let (oob, peek) = (
        wasixv2_abi::MsgFlags::MSG_OOB,
        wasixv2_abi::MsgFlags::MSG_PEEK,
    );
    let none = wasixv2_abi::MsgFlags(0);
    let (kernel, p, client, server) = tcp_pair(4301);
    let at_mark = |fd| {
        let got = block_on(kernel.ioctl(&p.caller(), fd, IoctlRequest::SIOCATMARK, 0x60));
        assert_eq!(got, Ok(0));
        i32::from_le_bytes(super::ipc::get(&p.memory.0, 0x60, 4).try_into().unwrap())
    };
    let pollpri = |fd: Fd| {
        let (_, revents) = super::ipc::ppoll(
            &kernel,
            &p.caller(),
            &p.memory.0,
            &[(fd.0, 2)],
            Some((0, 0)),
        );
        revents[0] & 2 != 0
    };
    // A blocking socket: `MSG_OOB` with nothing urgent fails at once.
    assert_eq!(errno(recv(&kernel, &p, client, 1, oob)), Errno::EINVAL);
    send(&kernel, &p, client, b"ab", none);
    send(&kernel, &p, client, b"cU", oob);
    send(&kernel, &p, client, b"de", none);
    assert!(pollpri(server));
    assert_eq!(at_mark(server), 0);
    let fionread = block_on(kernel.ioctl(&p.caller(), server, IoctlRequest::FIONREAD, 0x60));
    assert_eq!(fionread, Ok(0));
    assert_eq!(super::ipc::get(&p.memory.0, 0x60, 4), 3i32.to_le_bytes());
    assert_eq!(recv(&kernel, &p, server, 16, none), Ok(b"abc".to_vec()));
    assert_eq!(at_mark(server), 1);
    assert_eq!(recv(&kernel, &p, server, 16, oob | peek), Ok(b"U".to_vec()));
    assert_eq!(recv(&kernel, &p, server, 16, oob), Ok(b"U".to_vec()));
    assert!(!pollpri(server));
    assert_eq!(errno(recv(&kernel, &p, server, 16, oob)), Errno::EINVAL);
    assert_eq!(recv(&kernel, &p, server, 16, none), Ok(b"de".to_vec()));
    assert_eq!(at_mark(server), 0);
    let (r, _w) = pipe(&kernel, &p.caller(), OpenFlags(0));
    let pipe_mark = block_on(kernel.ioctl(&p.caller(), r, IoctlRequest::SIOCATMARK, 0x60));
    assert_eq!(errno(pipe_mark), Errno::ENOTTY);
}

// ---- dnotify ----

/// `si_band` of `POLL_MSG`: `POLLIN | POLLRDNORM | POLLMSG`.
const POLLMSG_BAND: i32 = 0x441;

/// Writes `path` with its NUL at `at`.
fn path_at(p: &P, at: u32, path: &str) -> GuestPtr<u8> {
    let mut name = path.as_bytes().to_vec();
    name.push(0);
    put(&p.memory.0, at, &name);
    GuestPtr::new(at)
}

fn mkdir(kernel: &Kernel, p: &P, path: &str) {
    let name = path_at(p, 0x800, path);
    assert_eq!(
        block_on(kernel.mkdirat(&p.caller(), CWD, name, 0o755)),
        Ok(0)
    );
}

fn create(kernel: &Kernel, p: &P, path: &str) {
    let fd = open(kernel, p, path, OpenFlags::O_CREAT | OpenFlags::O_WRONLY).unwrap();
    close(kernel, p, fd);
}

fn rename(kernel: &Kernel, p: &P, from: &str, to: &str) {
    let (from, to) = (path_at(p, 0x800, from), path_at(p, 0x880, to));
    let flags = wasixv2_abi::RenameFlags(0);
    let caller = p.caller();
    let renamed = kernel.renameat2(&caller, CWD, from, CWD, to, flags);
    assert_eq!(block_on(renamed), Ok(0));
}

/// A directory open as a descriptor whose `F_SETSIG` signal is `SIGRTMIN`, registered for
/// `arg`.
fn notify_dir(kernel: &Kernel, p: &P, path: &str, arg: u32) -> Fd {
    let fd = open(kernel, p, path, OpenFlags::O_RDONLY).unwrap();
    let rt = SIGRTMIN.0 as u32;
    assert_eq!(fcntl(kernel, p, fd, FcntlCmd::F_SETSIG, rt), Ok(0));
    assert_eq!(fcntl(kernel, p, fd, FcntlCmd::F_NOTIFY, arg), Ok(0));
    fd
}

/// The descriptors whose `SIGRTMIN` is pending, in order.
fn notified(kernel: &Kernel, p: &P) -> Vec<i32> {
    std::iter::from_fn(|| take(kernel, p, SIGRTMIN))
        .map(|info| {
            let (code, band, fd) = poll_info(&info);
            assert_eq!((code, band), (3, POLLMSG_BAND), "POLL_MSG");
            fd
        })
        .collect()
}

/// `F_NOTIFY` signals its file's owner, which it makes the caller's process unless the file
/// has one, once or with `DN_MULTISHOT` for good; an empty mask and closing the descriptor end
/// it. Without `F_SETSIG` the signal is plain `SIGIO`.
#[test]
fn dnotify_signals_the_directory_owner() {
    use wasixv2_abi::DnotifyFlags as Dn;
    let (kernel, p) = machine();
    block(&kernel, &p, &[SIGRTMIN, SIGIO]);
    mkdir(&kernel, &p, "/d");
    let d = notify_dir(&kernel, &p, "/d", Dn::DN_CREATE.0);
    let pid = getpid(&kernel, &p);
    assert_eq!(fcntl(&kernel, &p, d, FcntlCmd::F_GETOWN, 0), Ok(pid));
    create(&kernel, &p, "/d/a");
    create(&kernel, &p, "/d/b");
    assert_eq!(notified(&kernel, &p), [d.0], "one-shot");

    let multi = Dn::DN_CREATE.0 | Dn::DN_MULTISHOT.0;
    assert_eq!(fcntl(&kernel, &p, d, FcntlCmd::F_NOTIFY, multi), Ok(0));
    create(&kernel, &p, "/d/c");
    create(&kernel, &p, "/d/e");
    assert_eq!(notified(&kernel, &p), [d.0, d.0], "multishot");
    assert_eq!(
        fcntl(&kernel, &p, d, FcntlCmd::F_NOTIFY, Dn::DN_MULTISHOT.0),
        Ok(0)
    );
    create(&kernel, &p, "/d/f");
    assert_eq!(notified(&kernel, &p), [] as [i32; 0], "removed");

    // Closing any descriptor of the open file ends the registration (`dnotify_flush`).
    assert_eq!(fcntl(&kernel, &p, d, FcntlCmd::F_NOTIFY, multi), Ok(0));
    let dup = block_on(kernel.dup(&p.caller(), d)).unwrap();
    close(&kernel, &p, dup);
    create(&kernel, &p, "/d/g");
    assert_eq!(notified(&kernel, &p), [] as [i32; 0], "flushed");

    assert_eq!(fcntl(&kernel, &p, d, FcntlCmd::F_SETSIG, 0), Ok(0));
    assert_eq!(
        fcntl(&kernel, &p, d, FcntlCmd::F_NOTIFY, Dn::DN_CREATE.0),
        Ok(0)
    );
    create(&kernel, &p, "/d/h");
    assert!(take(&kernel, &p, SIGIO).is_some());
}

/// What each `DN_*` stands for: changes to the directory's entries and to the objects in it,
/// and to the directory itself (`fsnotify_parent`, `dnotify_handle_event`). `DN_RENAME` is a
/// rename within the directory; a move out is a deletion, a move in a creation.
#[test]
fn dnotify_events_follow_fsnotify() {
    use wasixv2_abi::DnotifyFlags as Dn;
    let (kernel, p) = machine();
    block(&kernel, &p, &[SIGRTMIN]);
    mkdir(&kernel, &p, "/d");
    mkdir(&kernel, &p, "/d/sub");
    create(&kernel, &p, "/d/f");
    let multi = Dn::DN_MULTISHOT.0;
    let modify = notify_dir(&kernel, &p, "/d", Dn::DN_MODIFY.0 | multi);
    let sub = notify_dir(&kernel, &p, "/d/sub", Dn::DN_ATTRIB.0 | multi);
    let f = open(&kernel, &p, "/d/f", OpenFlags::O_RDWR).unwrap();
    assert_eq!(write(&kernel, &p, f, b"x"), Ok(1));
    assert_eq!(notified(&kernel, &p), [modify.0]);
    // A change to `sub` reaches its own registration and its parent's.
    let attrib = notify_dir(&kernel, &p, "/d", Dn::DN_ATTRIB.0 | multi);
    let name = path_at(&p, 0x800, "/d/sub");
    assert_eq!(
        block_on(kernel.fchmodat(&p.caller(), CWD, name, 0o700)),
        Ok(0)
    );
    let mut got = notified(&kernel, &p);
    got.sort();
    let mut want = [attrib.0, sub.0];
    want.sort();
    assert_eq!(got, want);
    close(&kernel, &p, attrib);
    close(&kernel, &p, modify);
    close(&kernel, &p, sub);

    let access = notify_dir(&kernel, &p, "/d", Dn::DN_ACCESS.0 | multi);
    assert_eq!(
        block_on(kernel.pread64(&p.caller(), f, UserSliceMut::new(0x400, 1), 0)),
        Ok(1)
    );
    assert_eq!(notified(&kernel, &p), [access.0]);
    close(&kernel, &p, access);

    let rename_d = notify_dir(&kernel, &p, "/d", Dn::DN_RENAME.0 | multi);
    let rename_sub = notify_dir(&kernel, &p, "/d/sub", Dn::DN_RENAME.0 | multi);
    rename(&kernel, &p, "/d/f", "/d/sub/f");
    assert_eq!(
        notified(&kernel, &p),
        [] as [i32; 0],
        "not within one directory"
    );
    rename(&kernel, &p, "/d/sub/f", "/d/sub/g");
    assert_eq!(notified(&kernel, &p), [rename_sub.0]);
    close(&kernel, &p, rename_d);
    close(&kernel, &p, rename_sub);

    let delete = notify_dir(&kernel, &p, "/d", Dn::DN_DELETE.0 | multi);
    let create_sub = notify_dir(&kernel, &p, "/d/sub", Dn::DN_CREATE.0 | multi);
    rename(&kernel, &p, "/d/sub/g", "/d/g");
    assert_eq!(
        notified(&kernel, &p),
        [] as [i32; 0],
        "a move in creates, out deletes"
    );
    rename(&kernel, &p, "/d/g", "/d/sub/g");
    assert_eq!(notified(&kernel, &p), [delete.0, create_sub.0]);
    let name = path_at(&p, 0x800, "/d/sub/g");
    let caller = p.caller();
    let removed = kernel.unlinkat(&caller, CWD, name, wasixv2_abi::AtFlags(0));
    assert_eq!(block_on(removed), Ok(0));
    assert_eq!(notified(&kernel, &p), [] as [i32; 0], "not in /d");
}

/// `F_NOTIFY` needs a directory unless it removes, and `fs.dir-notify-enable`.
#[test]
fn dnotify_needs_a_directory_and_dir_notify_enable() {
    use wasixv2_abi::DnotifyFlags as Dn;
    let (kernel, p) = machine();
    create(&kernel, &p, "/f");
    let f = open(&kernel, &p, "/f", OpenFlags::O_RDONLY).unwrap();
    let create_ = Dn::DN_CREATE.0;
    assert_eq!(
        errno(fcntl(&kernel, &p, f, FcntlCmd::F_NOTIFY, create_)),
        Errno::ENOTDIR
    );
    assert_eq!(fcntl(&kernel, &p, f, FcntlCmd::F_NOTIFY, 0), Ok(0));
    let (r, _w) = pipe(&kernel, &p.caller(), OpenFlags(0));
    assert_eq!(
        errno(fcntl(&kernel, &p, r, FcntlCmd::F_NOTIFY, create_)),
        Errno::ENOTDIR
    );
    kernel.sysctls.set_dir_notify_enable(0);
    let d = open(&kernel, &p, "/", OpenFlags::O_RDONLY).unwrap();
    assert_eq!(
        errno(fcntl(&kernel, &p, d, FcntlCmd::F_NOTIFY, create_)),
        Errno::EINVAL
    );
    assert_eq!(
        errno(fcntl(&kernel, &p, d, FcntlCmd::F_NOTIFY, 0)),
        Errno::EINVAL
    );
}

// ---- leases ----

const F_RDLCK: u32 = 0;
const F_WRLCK: u32 = 1;
const F_UNLCK: u32 = 2;

/// A machine whose clock runs, so that lease breaks can wait and time out, with init and
/// one process run as root, and the file `/f` it owns.
fn lease_machine() -> (Kernel, P) {
    let host: Arc<dyn Host> = Arc::new(super::ptimer::RealHost(std::time::Instant::now()));
    let root = MemFs::new(Arc::new(HostClock(host.clone())), 0o755, 0, 0);
    let kernel = Kernel::new(host, Arc::new(root))
        .with_credentials(Credentials::new(0, 0))
        .with_init();
    let stdin = Arc::new(Input(Mutex::new(Vec::new())));
    let (kernel, task, _) = spawn_on(kernel, stdin, &[], &[]);
    let p = P::new(task);
    create(&kernel, &p, "/f");
    (kernel, p)
}

fn setlease(kernel: &Kernel, p: &P, fd: Fd, arg: u32) -> SysResult<i32> {
    fcntl(kernel, p, fd, FcntlCmd::F_SETLEASE, arg)
}

fn getlease(kernel: &Kernel, p: &P, fd: Fd) -> i32 {
    fcntl(kernel, p, fd, FcntlCmd::F_GETLEASE, 0).unwrap()
}

/// `generic_add_lease`'s checks: a read lease needs the file open for writing nowhere, a write
/// lease no open but the holder's (`lease_open_conflict`), and no other lease; only regular
/// files, of the caller's own unless it has `CAP_LEASE`; `F_UNLCK` without a lease is `EAGAIN`;
/// a new lease needs `fs.leases-enable`.
#[test]
fn a_lease_needs_no_conflicting_open() {
    let (kernel, p) = lease_machine();
    let r = open(&kernel, &p, "/f", OpenFlags::O_RDONLY).unwrap();
    assert_eq!(getlease(&kernel, &p, r), F_UNLCK as i32);
    assert_eq!(setlease(&kernel, &p, r, F_RDLCK), Ok(0));
    assert_eq!(getlease(&kernel, &p, r), F_RDLCK as i32);
    let pid = getpid(&kernel, &p);
    assert_eq!(fcntl(&kernel, &p, r, FcntlCmd::F_GETOWN, 0), Ok(pid));
    // Another read lease may join; a write lease may not, nor may a writer's read lease.
    let r2 = open(&kernel, &p, "/f", OpenFlags::O_RDONLY).unwrap();
    assert_eq!(setlease(&kernel, &p, r2, F_RDLCK), Ok(0));
    assert_eq!(errno(setlease(&kernel, &p, r2, F_WRLCK)), Errno::EAGAIN);
    assert_eq!(setlease(&kernel, &p, r2, F_UNLCK), Ok(0));
    assert_eq!(errno(setlease(&kernel, &p, r2, F_UNLCK)), Errno::EAGAIN);
    assert_eq!(setlease(&kernel, &p, r, F_UNLCK), Ok(0));
    assert_eq!(
        fcntl(&kernel, &p, r, FcntlCmd::F_GETOWN, 0),
        Ok(0),
        "f_delown"
    );
    // `r2` is open too, so no write lease for `r`.
    assert_eq!(errno(setlease(&kernel, &p, r, F_WRLCK)), Errno::EAGAIN);
    close(&kernel, &p, r2);
    assert_eq!(setlease(&kernel, &p, r, F_WRLCK), Ok(0));
    assert_eq!(getlease(&kernel, &p, r), F_WRLCK as i32);
    assert_eq!(setlease(&kernel, &p, r, F_UNLCK), Ok(0));

    let w = open(&kernel, &p, "/f", OpenFlags::O_WRONLY).unwrap();
    assert_eq!(errno(setlease(&kernel, &p, w, F_RDLCK)), Errno::EAGAIN);
    assert_eq!(errno(setlease(&kernel, &p, r, F_RDLCK)), Errno::EAGAIN);
    assert_eq!(errno(setlease(&kernel, &p, w, 7)), Errno::EINVAL);
    close(&kernel, &p, w);

    mkdir(&kernel, &p, "/d");
    let d = open(&kernel, &p, "/d", OpenFlags::O_RDONLY).unwrap();
    assert_eq!(errno(setlease(&kernel, &p, d, F_RDLCK)), Errno::EINVAL);
    let (pr, _pw) = pipe(&kernel, &p.caller(), OpenFlags(0));
    assert_eq!(errno(setlease(&kernel, &p, pr, F_RDLCK)), Errno::EINVAL);
    assert_eq!(getlease(&kernel, &p, pr), F_UNLCK as i32);

    let procfs = kernel.procfs();
    let mount = kernel
        .vfs()
        .mount(b"/proc", procfs, MountFlags::default(), true);
    block_on(mount).unwrap();
    let status = open(&kernel, &p, "/proc/self/status", OpenFlags::O_RDONLY).unwrap();
    assert_eq!(errno(setlease(&kernel, &p, status, F_RDLCK)), Errno::EINVAL);

    kernel.sysctls.set_leases_enable(0);
    assert_eq!(errno(setlease(&kernel, &p, r, F_RDLCK)), Errno::EINVAL);
    kernel.sysctls.set_leases_enable(1);

    // Someone else's file needs `CAP_LEASE`, which a uid of 1 has not.
    let caller = p.caller();
    let set = kernel.setresuid(&caller, 1, 1, 1);
    assert_eq!(block_on(set), Ok(0));
    assert_eq!(errno(setlease(&kernel, &p, r, F_RDLCK)), Errno::EACCES);
}

/// An open that conflicts with a lease tells the holder, by its `F_SETSIG` signal with
/// `POLL_MSG` and the lease's descriptor, and with `O_NONBLOCK` fails with `EWOULDBLOCK`. A
/// reader only wants a write lease made a read lease (`F_GETLEASE` says what the lease is to
/// become); a writer wants it gone.
#[test]
fn a_conflicting_open_breaks_the_lease() {
    let (kernel, p) = lease_machine();
    block(&kernel, &p, &[SIGRTMIN]);
    let r = open(&kernel, &p, "/f", OpenFlags::O_RDONLY).unwrap();
    let rt = SIGRTMIN.0 as u32;
    assert_eq!(fcntl(&kernel, &p, r, FcntlCmd::F_SETSIG, rt), Ok(0));
    assert_eq!(setlease(&kernel, &p, r, F_WRLCK), Ok(0));
    let nonblock = OpenFlags::O_NONBLOCK;
    let opened = open(&kernel, &p, "/f", OpenFlags::O_RDONLY | nonblock);
    assert_eq!(errno(opened), Errno::EWOULDBLOCK);
    let info = take(&kernel, &p, SIGRTMIN).expect("the break");
    assert_eq!(poll_info(&info), (3, POLLMSG_BAND, r.0));
    assert_eq!(getlease(&kernel, &p, r), F_RDLCK as i32);
    // Told once per break.
    let opened = open(&kernel, &p, "/f", OpenFlags::O_RDONLY | nonblock);
    assert_eq!(errno(opened), Errno::EWOULDBLOCK);
    assert!(take(&kernel, &p, SIGRTMIN).is_none());
    assert_eq!(setlease(&kernel, &p, r, F_RDLCK), Ok(0));
    let reader = open(&kernel, &p, "/f", OpenFlags::O_RDONLY | nonblock).unwrap();
    close(&kernel, &p, reader);

    let opened = open(&kernel, &p, "/f", OpenFlags::O_WRONLY | nonblock);
    assert_eq!(errno(opened), Errno::EWOULDBLOCK);
    assert!(take(&kernel, &p, SIGRTMIN).is_some());
    assert_eq!(getlease(&kernel, &p, r), F_UNLCK as i32);
    // A lease being broken for a writer keeps new leases away.
    let r2 = open(&kernel, &p, "/f", OpenFlags::O_RDONLY).unwrap();
    assert_eq!(errno(setlease(&kernel, &p, r2, F_RDLCK)), Errno::EAGAIN);
    // The last close takes the lease.
    close(&kernel, &p, r);
    let writer = open(&kernel, &p, "/f", OpenFlags::O_WRONLY | nonblock).unwrap();
    close(&kernel, &p, writer);

    // A downgrade not made in time is made for the holder (`time_out_leases`).
    kernel.sysctls.set_lease_break_time(1);
    close(&kernel, &p, r2);
    let r = open(&kernel, &p, "/f", OpenFlags::O_RDONLY).unwrap();
    assert_eq!(setlease(&kernel, &p, r, F_WRLCK), Ok(0));
    let opened = open(&kernel, &p, "/f", OpenFlags::O_RDONLY | nonblock);
    assert_eq!(errno(opened), Errno::EWOULDBLOCK);
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(getlease(&kernel, &p, r), F_RDLCK as i32);
    let reader = open(&kernel, &p, "/f", OpenFlags::O_RDONLY | nonblock);
    assert!(reader.is_ok());
}

/// A blocking open or truncation waits for the holder: a writer counts as open for writing
/// meanwhile, so the holder cannot make its lease a read lease, only give it up; a signal
/// ends the wait with a restart; and once `fs.lease-break-time` is up the lease goes, with the
/// file's owner.
#[test]
fn a_blocking_open_waits_for_the_holder() {
    let (kernel, p) = lease_machine();
    block(&kernel, &p, &[SIGIO]);
    let r = open(&kernel, &p, "/f", OpenFlags::O_RDONLY).unwrap();
    assert_eq!(setlease(&kernel, &p, r, F_WRLCK), Ok(0));
    let (child, _) = fork(&kernel, &p, 0);
    let child_task = child.task;
    std::thread::scope(|scope| {
        let opener = scope.spawn(|| {
            let child = child;
            let opened = open(&kernel, &child, "/f", OpenFlags::O_WRONLY);
            (child, opened)
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while take(&kernel, &p, SIGIO).is_none() {
            assert!(std::time::Instant::now() < deadline, "no SIGIO");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(errno(setlease(&kernel, &p, r, F_RDLCK)), Errno::EAGAIN);
        assert!(!opener.is_finished());
        assert_eq!(setlease(&kernel, &p, r, F_UNLCK), Ok(0));
        let (child, opened) = opener.join().unwrap();
        assert!(opened.is_ok());
        close(&kernel, &child, opened.unwrap());

        // A truncation waits too, until a signal for the waiter.
        assert_eq!(setlease(&kernel, &p, r, F_WRLCK), Ok(0));
        let kernel = &kernel;
        let truncater = scope.spawn(move || {
            let name = path_at(&child, 0x800, "/f");
            let caller = child.caller();
            block_on(kernel.truncate64(&caller, name, 0))
        });
        while take(kernel, &p, SIGIO).is_none() {
            assert!(std::time::Instant::now() < deadline, "no SIGIO");
            std::thread::sleep(Duration::from_millis(5));
        }
        kernel.kill(child_task, Signal::SIGUSR1).unwrap();
        assert_eq!(truncater.join().unwrap(), Err(SysErr::Restart));
    });

    // Its time up, the lease goes without the holder. A break keeps the time it was given.
    assert_eq!(setlease(&kernel, &p, r, F_UNLCK), Ok(0));
    kernel.sysctls.set_lease_break_time(1);
    assert_eq!(setlease(&kernel, &p, r, F_WRLCK), Ok(0));
    let (child, _) = fork(&kernel, &p, 0);
    let started = std::time::Instant::now();
    let opened = open(&kernel, &child, "/f", OpenFlags::O_RDWR);
    assert!(opened.is_ok());
    assert!(started.elapsed() >= Duration::from_secs(1));
    assert_eq!(getlease(&kernel, &p, r), F_UNLCK as i32);
    assert_eq!(fcntl(&kernel, &p, r, FcntlCmd::F_GETOWN, 0), Ok(0));
}
