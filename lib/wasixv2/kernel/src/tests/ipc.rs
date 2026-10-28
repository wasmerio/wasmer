//! Pipes, eventfd, poll, select and epoll.

use wasixv2_abi::{EventfdFlags, FdPair};
use wasixv2_wait::{PollMask, Pollable};

use super::*;
use crate::sync::MutexExt;

pub(super) fn errno<T: std::fmt::Debug>(result: SysResult<T>) -> Errno {
    match result {
        Err(SysErr::Errno(errno)) => errno,
        other => panic!("expected an errno, got {other:?}"),
    }
}

/// A kernel with one task and 64 KiB of zeroed guest memory.
pub(super) fn machine() -> (Kernel, TaskId, FlatMemory) {
    let (kernel, task, _) = setup(b"");
    (kernel, task, FlatMemory(RefCell::new(vec![0; 65536])))
}

pub(super) fn pipe(kernel: &Kernel, caller: &Caller<'_>, flags: OpenFlags) -> (Fd, Fd) {
    let memory = caller.memory();
    assert_eq!(
        block_on(kernel.pipe2(caller, FdPair::new(0x100), flags)),
        Ok(0)
    );
    let mut raw = [0; 8];
    block_on(memory.read(0x100, &mut raw)).unwrap();
    let fd = |i: usize| Fd(i32::from_le_bytes(raw[i..i + 4].try_into().unwrap()));
    (fd(0), fd(4))
}

pub(super) fn put(memory: &FlatMemory, addr: u32, data: &[u8]) {
    memory.0.borrow_mut()[addr as usize..][..data.len()].copy_from_slice(data);
}

pub(super) fn get(memory: &FlatMemory, addr: u32, len: usize) -> Vec<u8> {
    memory.0.borrow()[addr as usize..][..len].to_vec()
}

#[test]
fn pipe_moves_bytes_and_reports_eof_after_the_writer_closes() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (r, w) = pipe(&kernel, &caller, OpenFlags::O_NONBLOCK);
    assert_eq!((r, w), (Fd(3), Fd(4)));
    put(&memory, 0x200, b"abcde");

    assert_eq!(
        block_on(kernel.write(&caller, w, UserSlice::new(0x200, 5))),
        Ok(5)
    );
    assert_eq!(
        errno(block_on(kernel.read(
            &caller,
            w,
            UserSliceMut::new(0x300, 5)
        ))),
        Errno::EBADF
    );
    assert_eq!(
        block_on(kernel.read(&caller, r, UserSliceMut::new(0x300, 3))),
        Ok(3)
    );
    assert_eq!(get(&memory, 0x300, 3), b"abc");
    assert_eq!(
        block_on(kernel.read(&caller, r, UserSliceMut::new(0x300, 9))),
        Ok(2)
    );
    assert_eq!(
        errno(block_on(kernel.read(
            &caller,
            r,
            UserSliceMut::new(0x300, 9)
        ))),
        Errno::EAGAIN
    );
    assert_eq!(block_on(kernel.close(&caller, w)), Ok(0));
    assert_eq!(
        block_on(kernel.read(&caller, r, UserSliceMut::new(0x300, 9))),
        Ok(0)
    );
}

#[test]
fn pipe_write_without_readers_is_epipe() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (r, w) = pipe(&kernel, &caller, OpenFlags(0));
    assert_eq!(block_on(kernel.close(&caller, r)), Ok(0));
    assert_eq!(
        errno(block_on(kernel.write(&caller, w, UserSlice::new(0x200, 1)))),
        Errno::EPIPE
    );
}

#[test]
fn full_pipe_takes_a_partial_write_but_not_a_partial_atomic_one() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (_r, w) = pipe(&kernel, &caller, OpenFlags::O_NONBLOCK);
    let fcntl = |cmd, arg| block_on(kernel.fcntl64(&caller, w, cmd, arg));
    assert_eq!(fcntl(FcntlCmd::F_GETPIPE_SZ, 0), Ok(65536));
    assert_eq!(fcntl(FcntlCmd::F_SETPIPE_SZ, 5000), Ok(8192));

    assert_eq!(
        block_on(kernel.write(&caller, w, UserSlice::new(0, 6000))),
        Ok(6000)
    );
    // 2192 bytes free: an atomic 4096-byte write must not split, a larger one may.
    let atomic = block_on(kernel.write(&caller, w, UserSlice::new(0, 4096)));
    assert_eq!(errno(atomic), Errno::EAGAIN);
    assert_eq!(
        block_on(kernel.write(&caller, w, UserSlice::new(0, 5000))),
        Ok(2192)
    );
    assert_eq!(
        fcntl(FcntlCmd::F_SETPIPE_SZ, 4096),
        Err(SysErr::Errno(Errno::EBUSY))
    );
}

#[test]
fn blocking_pipe_read_waits_for_a_writer_on_another_thread() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (r, w) = pipe(&kernel, &caller, OpenFlags(0));
    let writer = kernel.file(task, w).unwrap();

    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(20));
            assert_eq!(
                block_on(writer.write(&wasixv2_wait::NoInterrupt, &Default::default(), b"xy")),
                Ok(2)
            );
        });
        assert_eq!(
            block_on(kernel.read(&caller, r, UserSliceMut::new(0x300, 9))),
            Ok(2)
        );
    });
    assert_eq!(get(&memory, 0x300, 2), b"xy");
}

/// Writes one iovec `{base, len}` at `at`.
fn iovec(memory: &FlatMemory, at: u32, base: u32, len: u32) -> wasixv2_abi::ConstIovecArray {
    put(memory, at, &base.to_le_bytes());
    put(memory, at + 4, &len.to_le_bytes());
    GuestPtr::new(at)
}

#[test]
fn a_blocked_readv_or_writev_ends_on_a_signal() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (r, w) = pipe(&kernel, &caller, OpenFlags(0));
    let vec = iovec(&memory, 0x80, 0x300, 9);

    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(20));
            kernel.kill(task, Signal::SIGUSR1).unwrap();
        });
        assert_eq!(
            block_on(kernel.readv(&caller, r, vec, 1)),
            Err(SysErr::Restart)
        );
    });

    // The signal is still pending, so a writev that has to wait for room gives up at once,
    // with what it got in before.
    let fill = iovec(&memory, 0x80, 0, 65536 + 1);
    assert_eq!(block_on(kernel.writev(&caller, w, fill, 1)), Ok(65536));
    let one = iovec(&memory, 0x80, 0, 1);
    assert_eq!(
        block_on(kernel.writev(&caller, w, one, 1)),
        Err(SysErr::Restart)
    );
}

#[test]
fn a_blocking_write_to_a_pipe_writes_everything() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (r, w) = pipe(&kernel, &caller, OpenFlags(0));
    let reader = kernel.file(task, r).unwrap();
    // More than a pipe holds and more than one kernel step: the write waits for the reader.
    const LEN: usize = 60_000;
    let total = std::thread::scope(|scope| {
        let drain = scope.spawn(|| {
            let mut got = 0;
            let mut buf = vec![0; 8192];
            while got < 3 * LEN {
                let n = block_on(reader.read(&wasixv2_wait::NoInterrupt, &mut buf)).unwrap();
                got += n;
            }
            got
        });
        let vec = iovec(&memory, 0x80, 0, LEN as u32);
        iovec(&memory, 0x88, 0, LEN as u32);
        iovec(&memory, 0x90, 0, LEN as u32);
        assert_eq!(
            block_on(kernel.writev(&caller, w, vec, 3)),
            Ok(3 * LEN as u32)
        );
        drain.join().unwrap()
    });
    assert_eq!(total, 3 * LEN);
}

#[test]
fn epipe_raises_sigpipe_only_on_byte_streams() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let sigpipe = 1 << (Signal::SIGPIPE.0 - 1);
    let pending = || kernel.task_signals(task).has_pending(sigpipe);

    // A seqpacket socket whose peer is gone fails with EPIPE, and that is all.
    let pair = FdPair::new(0x100);
    let unix = wasixv2_abi::AddressFamily(1);
    const SOCK_SEQPACKET: i32 = 5;
    let made = kernel.socketpair(&caller, unix, SOCK_SEQPACKET, 0, pair);
    assert_eq!(block_on(made), Ok(0));
    let fd = |at| Fd(i32::from_le_bytes(get(&memory, at, 4).try_into().unwrap()));
    assert_eq!(block_on(kernel.close(&caller, fd(0x104))), Ok(0));
    let sent = block_on(kernel.write(&caller, fd(0x100), UserSlice::new(0, 4)));
    assert_eq!(errno(sent), Errno::EPIPE);
    assert!(!pending());

    // A pipe without readers raises it.
    let (r, w) = pipe(&kernel, &caller, OpenFlags(0));
    assert_eq!(block_on(kernel.close(&caller, r)), Ok(0));
    let written = block_on(kernel.write(&caller, w, UserSlice::new(0, 4)));
    assert_eq!(errno(written), Errno::EPIPE);
    assert!(pending());
}

#[test]
fn a_write_cut_short_by_the_last_reader_leaving_raises_sigpipe() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 1 << 17]));
    let caller = Caller::new(task, &memory);
    let sigpipe = 1 << (Signal::SIGPIPE.0 - 1);
    let signals = kernel.task_signals(task);
    // Each write fills the pipe and then waits for room, until the reader leaves: once inside
    // one step of the object, once between the kernel's steps. Both return what went in and
    // raise SIGPIPE, as `pipe_write` does whenever `!pipe->readers`.
    for (size, len) in [(4096, 8192), (65536, 65536 + 100)] {
        let (r, w) = pipe(&kernel, &caller, OpenFlags(0));
        let set_size = kernel.fcntl64(&caller, w, FcntlCmd::F_SETPIPE_SZ, size);
        assert_eq!(block_on(set_size), Ok(size as i32));
        let written = std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(20));
                drop(kernel.files(task).lock_or_recover().remove(r));
            });
            block_on(kernel.write(&caller, w, UserSlice::new(0, len)))
        });
        assert_eq!(written, Ok(size));
        assert!(signals.has_pending(sigpipe), "pipe size {size}");
        assert!(signals.dequeue_from(sigpipe).is_some());
        assert_eq!(block_on(kernel.close(&caller, w)), Ok(0));
    }
}

#[test]
fn a_stream_read_takes_all_that_is_buffered_and_a_datagram_whole() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 1 << 20]));
    let caller = Caller::new(task, &memory);
    // 200 KiB in a 1 MiB pipe come out in one read, as from `pipe_read`, not in 64 KiB steps.
    let (r, w) = pipe(&kernel, &caller, OpenFlags::O_NONBLOCK);
    let set_size = kernel.fcntl64(&caller, w, FcntlCmd::F_SETPIPE_SZ, 1 << 20);
    assert_eq!(block_on(set_size), Ok(1 << 20));
    const LEN: u32 = 200 << 10;
    put(&memory, 0, &vec![b'p'; LEN as usize]);
    let write = |fd| block_on(kernel.write(&caller, fd, UserSlice::new(0, LEN)));
    assert_eq!(write(w), Ok(LEN));
    let read = |fd| block_on(kernel.read(&caller, fd, UserSliceMut::new(LEN, 512 << 10)));
    assert_eq!(read(r), Ok(LEN));
    assert!(get(&memory, LEN, LEN as usize).iter().all(|&b| b == b'p'));

    // A Unix datagram larger than one step is read whole.
    let pair = FdPair::new(0x100);
    let unix = wasixv2_abi::AddressFamily(1);
    const SOCK_DGRAM: i32 = 2;
    let made = kernel.socketpair(&caller, unix, SOCK_DGRAM, 0, pair);
    assert_eq!(block_on(made), Ok(0));
    let fd = |at| Fd(i32::from_le_bytes(get(&memory, at, 4).try_into().unwrap()));
    let (a, b) = (fd(0x100), fd(0x104));
    const DGRAM: u32 = 100 << 10;
    let sent = block_on(kernel.write(&caller, a, UserSlice::new(0, DGRAM)));
    assert_eq!(sent, Ok(DGRAM));
    assert_eq!(read(b), Ok(DGRAM));
}

#[test]
fn readv_copies_out_up_to_a_fault() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (r, w) = pipe(&kernel, &caller, OpenFlags::O_NONBLOCK);
    put(&memory, 0x200, b"abcdef");
    assert_eq!(
        block_on(kernel.write(&caller, w, UserSlice::new(0x200, 6))),
        Ok(6)
    );
    // Three bytes fit the first buffer; the second lies outside guest memory.
    let vec = iovec(&memory, 0x80, 0x300, 3);
    iovec(&memory, 0x88, 1 << 30, 3);
    assert_eq!(block_on(kernel.readv(&caller, r, vec, 2)), Ok(3));
    assert_eq!(get(&memory, 0x300, 3), b"abc");
}

#[test]
fn preadv_and_pwritev_reject_negative_offsets() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (r, w) = pipe(&kernel, &caller, OpenFlags::O_NONBLOCK);
    let vec = iovec(&memory, 0x80, 0x300, 1);
    // `do_preadv` checks the offset before it looks at the descriptor; only the v2 calls give
    // -1 a meaning.
    assert_eq!(
        errno(block_on(kernel.preadv(&caller, r, vec, 1, -1))),
        Errno::EINVAL
    );
    assert_eq!(
        errno(block_on(kernel.pwritev(&caller, w, vec, 1, -5))),
        Errno::EINVAL
    );
    assert_eq!(
        errno(block_on(kernel.preadv2(
            &caller,
            r,
            vec,
            1,
            -2,
            RwfFlags(0)
        ))),
        Errno::EINVAL
    );
}

/// `fstat` through `statx(fd, "", AT_EMPTY_PATH)`.
fn stat_of(
    kernel: &Kernel,
    caller: &Caller<'_>,
    memory: &FlatMemory,
    fd: Fd,
) -> wasixv2_abi::Statx {
    put(memory, 0x2ff0, b"\0");
    let (name, out) = (GuestPtr::new(0x2ff0), GuestPtrMut::new(0x3000));
    let mask = wasixv2_abi::StatxMask(0x7ff);
    let flags = wasixv2_abi::AtFlags::AT_EMPTY_PATH;
    block_on(kernel.statx(caller, fd, name, flags, mask, out)).unwrap();
    block_on(wasixv2_syscalls::read_value(memory, GuestPtr::new(0x3000))).unwrap()
}

#[test]
fn fstat_shows_each_object_its_pseudo_inode() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    const S_IFMT: u16 = 0o170000;
    let (r, w) = pipe(&kernel, &caller, OpenFlags(0));
    let (other, _) = pipe(&kernel, &caller, OpenFlags(0));
    let (rs, ws, os) = (
        stat_of(&kernel, &caller, &memory, r),
        stat_of(&kernel, &caller, &memory, w),
        stat_of(&kernel, &caller, &memory, other),
    );
    // Both ends of a pipe are one FIFO inode; another pipe is another inode.
    assert_eq!(rs.stx_mode, 0o010600);
    assert_eq!(
        (rs.stx_ino, rs.stx_dev_minor),
        (ws.stx_ino, ws.stx_dev_minor)
    );
    assert_ne!(rs.stx_ino, os.stx_ino);

    let sockets = FdPair::new(0x100);
    let unix = wasixv2_abi::AddressFamily(1);
    assert_eq!(
        block_on(kernel.socketpair(&caller, unix, 1, 0, sockets)),
        Ok(0)
    );
    let fd = |at| Fd(i32::from_le_bytes(get(&memory, at, 4).try_into().unwrap()));
    let (a, b) = (
        stat_of(&kernel, &caller, &memory, fd(0x100)),
        stat_of(&kernel, &caller, &memory, fd(0x104)),
    );
    assert_eq!(a.stx_mode & S_IFMT, 0o140000);
    assert_ne!(a.stx_ino, b.stx_ino);
    assert_ne!(a.stx_dev_minor, rs.stx_dev_minor);

    // eventfd, epoll and friends share one anon inode with no file type, made by root.
    let efd = block_on(kernel.eventfd2(&caller, 0, EventfdFlags(0))).unwrap();
    let anon = stat_of(&kernel, &caller, &memory, efd);
    assert_eq!((anon.stx_mode, anon.stx_uid), (0o600, 0));
    let epfd = block_on(kernel.epoll_create1(&caller, wasixv2_abi::EpollCreateFlags(0)));
    assert_eq!(
        stat_of(&kernel, &caller, &memory, epfd.unwrap()).stx_ino,
        anon.stx_ino
    );
}

#[test]
fn a_nonblocking_fifo_reader_hangs_up_only_after_a_writer_left() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    put(&memory, 0x40, b"fifo\0");
    let mode: Mode = 0o010600;
    assert_eq!(
        block_on(kernel.mknodat(&caller, Fd(-100), GuestPtr::new(0x40), mode, 0)),
        Ok(0)
    );
    let open = |flags: OpenFlags| {
        block_on(kernel.openat(&caller, Fd(-100), GuestPtr::new(0x40), flags, 0)).unwrap()
    };
    let reader = open(OpenFlags::O_RDONLY | OpenFlags::O_NONBLOCK);
    let mask = || kernel.file(task, reader).unwrap().poll(None);
    // No writer has come yet: neither readable nor hung up (`f_pipe`).
    assert_eq!(mask(), PollMask::empty());

    let writer = open(OpenFlags::O_WRONLY | OpenFlags::O_NONBLOCK);
    assert_eq!(mask(), PollMask::empty());
    assert_eq!(block_on(kernel.close(&caller, writer)), Ok(0));
    assert_eq!(mask(), PollMask::HUP);
}

#[test]
fn eventfd_counts_and_semaphores() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let flags = EventfdFlags::EFD_SEMAPHORE | EventfdFlags::EFD_NONBLOCK;
    let fd = block_on(kernel.eventfd2(&caller, 2, flags)).unwrap();
    let read = || block_on(kernel.read(&caller, fd, UserSliceMut::new(0x300, 8)));

    assert_eq!(read(), Ok(8));
    assert_eq!(get(&memory, 0x300, 8), 1u64.to_le_bytes());
    assert_eq!(read(), Ok(8));
    assert_eq!(errno(read()), Errno::EAGAIN);
    put(&memory, 0x200, &u64::MAX.to_le_bytes());
    assert_eq!(
        errno(block_on(kernel.write(
            &caller,
            fd,
            UserSlice::new(0x200, 8)
        ))),
        Errno::EINVAL
    );
    put(&memory, 0x200, &5u64.to_le_bytes());
    // `eventfd_write` takes exactly 8 bytes: more is EINVAL too, and adds nothing.
    for len in [4, 16] {
        let write = kernel.write(&caller, fd, UserSlice::new(0x200, len));
        assert_eq!(errno(block_on(write)), Errno::EINVAL);
    }
    assert_eq!(errno(read()), Errno::EAGAIN);
    assert_eq!(
        block_on(kernel.write(&caller, fd, UserSlice::new(0x200, 8))),
        Ok(8)
    );
    assert_eq!(
        errno(block_on(kernel.read(
            &caller,
            fd,
            UserSliceMut::new(0x300, 4)
        ))),
        Errno::EINVAL
    );
    assert_eq!(
        errno(block_on(kernel.eventfd2(&caller, 0, EventfdFlags(4)))),
        Errno::EINVAL
    );
}

/// Writes pollfds at 0x400 and runs ppoll with the timespec at 0x500 (`None`: no timeout).
fn ppoll(
    kernel: &Kernel,
    caller: &Caller<'_>,
    memory: &FlatMemory,
    fds: &[(i32, u16)],
    timeout: Option<(i64, i64)>,
) -> (SysResult<i32>, Vec<u16>) {
    for (i, &(fd, events)) in fds.iter().enumerate() {
        let at = 0x400 + i as u32 * 8;
        put(memory, at, &fd.to_le_bytes());
        put(memory, at + 4, &events.to_le_bytes());
        put(memory, at + 6, &0xffffu16.to_le_bytes());
    }
    let tsp = match timeout {
        None => 0,
        Some((sec, nsec)) => {
            put(memory, 0x500, &sec.to_le_bytes());
            put(memory, 0x508, &nsec.to_le_bytes());
            0x500
        }
    };
    let result = block_on(kernel.ppoll_time64(
        caller,
        GuestPtrMut::new(0x400),
        fds.len() as u32,
        GuestPtrMut::new(tsp),
        GuestPtr::NULL,
        8,
    ));
    let revents = (0..fds.len() as u32)
        .map(|i| u16::from_le_bytes(get(memory, 0x406 + i * 8, 2).try_into().unwrap()))
        .collect();
    (result, revents)
}

#[test]
fn ppoll_reports_readiness_nval_and_skips_negative_fds() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (r, w) = pipe(&kernel, &caller, OpenFlags(0));
    const IN: u16 = 0x1;
    const OUT: u16 = 0x4;

    let (result, revents) = ppoll(&kernel, &caller, &memory, &[(r.0, IN), (w.0, OUT)], None);
    assert_eq!(result, Ok(1));
    assert_eq!(revents, [0, OUT]);

    assert_eq!(
        block_on(kernel.write(&caller, w, UserSlice::new(0, 3))),
        Ok(3)
    );
    let fds = [(r.0, IN | OUT), (-1, IN), (99, 0)];
    let (result, revents) = ppoll(&kernel, &caller, &memory, &fds, Some((0, 0)));
    assert_eq!(result, Ok(2));
    assert_eq!(revents, [IN, 0, 0x20]);

    // POLLHUP is reported even when not asked for.
    assert_eq!(block_on(kernel.close(&caller, w)), Ok(0));
    let (_, revents) = ppoll(&kernel, &caller, &memory, &[(r.0, 0)], Some((0, 0)));
    assert_eq!(revents, [0x10]);
}

#[test]
fn ppoll_times_out_and_rejects_bad_timeouts() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (r, _w) = pipe(&kernel, &caller, OpenFlags(0));

    // The test host's sleeps end at once.
    let (result, _) = ppoll(&kernel, &caller, &memory, &[(r.0, 1)], Some((5, 0)));
    assert_eq!(result, Ok(0));
    let (result, _) = ppoll(&kernel, &caller, &memory, &[(r.0, 1)], Some((0, -1)));
    assert_eq!(errno(result), Errno::EINVAL);
    let bad_sigset = block_on(kernel.ppoll_time64(
        &caller,
        GuestPtrMut::new(0x400),
        0,
        GuestPtrMut::NULL,
        GuestPtr::new(0x600),
        4,
    ));
    assert_eq!(errno(bad_sigset), Errno::EINVAL);
}

#[test]
fn ppoll_blocks_until_another_thread_writes() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (r, w) = pipe(&kernel, &caller, OpenFlags(0));
    let writer = kernel.file(task, w).unwrap();

    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(20));
            assert_eq!(
                block_on(writer.write(&wasixv2_wait::NoInterrupt, &Default::default(), b"!")),
                Ok(1)
            );
        });
        let (result, revents) = ppoll(&kernel, &caller, &memory, &[(r.0, 1)], None);
        assert_eq!((result, revents), (Ok(1), vec![1]));
    });
}

#[test]
fn pselect_checks_descriptors_and_fills_the_sets() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (r, w) = pipe(&kernel, &caller, OpenFlags(0));
    let set = |addr: u32, fds: &[Fd]| {
        let mut bits = [0u8; 128];
        for fd in fds {
            bits[fd.0 as usize / 8] |= 1 << (fd.0 % 8);
        }
        put(&memory, addr, &bits);
    };
    let select = |n| {
        put(&memory, 0x500, &[0; 16]);
        block_on(kernel.pselect6_time64(
            &caller,
            n,
            GuestPtrMut::new(0x600),
            GuestPtrMut::new(0x700),
            GuestPtrMut::NULL,
            GuestPtrMut::new(0x500),
            GuestPtr::NULL,
        ))
    };

    set(0x600, &[r]);
    set(0x700, &[w]);
    assert_eq!(select(8), Ok(1));
    assert_eq!(get(&memory, 0x600, 1)[0], 0);
    assert_eq!(get(&memory, 0x700, 1)[0], 1 << w.0);

    set(0x600, &[Fd(7)]);
    set(0x700, &[]);
    assert_eq!(errno(select(8)), Errno::EBADF);
    // Descriptors at or past `n` are not looked at.
    assert_eq!(select(7), Ok(0));
    assert_eq!(errno(select(-1)), Errno::EINVAL);
}

mod epoll {
    use wasixv2_abi::{EpollCreateFlags, EpollCtlOp, EpollEvents};

    use super::*;

    const IN: u32 = EpollEvents::EPOLLIN.0;
    const OUT: u32 = EpollEvents::EPOLLOUT.0;
    const HUP: u32 = EpollEvents::EPOLLHUP.0;
    const ET: u32 = EpollEvents::EPOLLET.0;
    const ONESHOT: u32 = EpollEvents::EPOLLONESHOT.0;
    const EXCLUSIVE: u32 = EpollEvents::EPOLLEXCLUSIVE.0;

    struct Fixture {
        kernel: Kernel,
        task: TaskId,
        memory: FlatMemory,
    }

    impl Fixture {
        fn new() -> Self {
            let (kernel, task, memory) = machine();
            Self {
                kernel,
                task,
                memory,
            }
        }

        fn caller(&self) -> Caller<'_> {
            Caller::new(self.task, &self.memory)
        }

        fn create(&self) -> Fd {
            block_on(
                self.kernel
                    .epoll_create1(&self.caller(), EpollCreateFlags(0)),
            )
            .unwrap()
        }

        fn ctl(&self, epfd: Fd, op: EpollCtlOp, fd: Fd, events: u32) -> SysResult<i32> {
            put(&self.memory, 0x800, &events.to_le_bytes());
            put(&self.memory, 0x808, &(fd.0 as u64).to_le_bytes());
            let event = GuestPtr::new(0x800);
            block_on(self.kernel.epoll_ctl(&self.caller(), epfd, op, fd, event))
        }

        fn add(&self, epfd: Fd, fd: Fd, events: u32) {
            assert_eq!(self.ctl(epfd, EpollCtlOp::EPOLL_CTL_ADD, fd, events), Ok(0));
        }

        /// (events, data) pairs from a zero-timeout wait.
        fn wait(&self, epfd: Fd, timeout: i32) -> Vec<(u32, u64)> {
            let n = block_on(self.kernel.epoll_pwait(
                &self.caller(),
                epfd,
                GuestPtrMut::new(0x900),
                8,
                timeout,
                GuestPtr::NULL,
                8,
            ))
            .unwrap();
            (0..n as u32)
                .map(|i| {
                    let at = 0x900 + i * 16;
                    let events = u32::from_le_bytes(get(&self.memory, at, 4).try_into().unwrap());
                    let data = u64::from_le_bytes(get(&self.memory, at + 8, 8).try_into().unwrap());
                    (events, data)
                })
                .collect()
        }

        fn write(&self, fd: Fd, len: u32) {
            let n = block_on(
                self.kernel
                    .write(&self.caller(), fd, UserSlice::new(0, len)),
            );
            assert_eq!(n, Ok(len));
        }

        fn read(&self, fd: Fd, len: u32) -> SysResult<u32> {
            block_on(
                self.kernel
                    .read(&self.caller(), fd, UserSliceMut::new(0x100, len)),
            )
        }

        fn close(&self, fd: Fd) {
            assert_eq!(block_on(self.kernel.close(&self.caller(), fd)), Ok(0));
        }

        fn dup(&self, fd: Fd) -> Fd {
            let file = self.kernel.file(self.task, fd).unwrap();
            let files = self.kernel.files(self.task);
            files.lock_or_recover().install(file, false).unwrap()
        }
    }

    #[test]
    fn events_that_fault_on_the_way_out_stay_for_the_next_wait() {
        let t = Fixture::new();
        let (r1, w1) = pipe(&t.kernel, &t.caller(), OpenFlags::O_NONBLOCK);
        let (r2, w2) = pipe(&t.kernel, &t.caller(), OpenFlags::O_NONBLOCK);
        let ep = t.create();
        t.add(ep, r1, IN | ONESHOT);
        t.add(ep, r2, IN | ET);
        t.write(w1, 1);
        t.write(w2, 1);
        // The array lies past the end of memory: nothing is copied out, and nothing is lost.
        let past_end = GuestPtrMut::new(65536 - 8);
        let caller = t.caller();
        let wait = t
            .kernel
            .epoll_pwait(&caller, ep, past_end, 8, 0, GuestPtr::NULL, 8);
        assert_eq!(errno(block_on(wait)), Errno::EFAULT);

        let mut got = t.wait(ep, 0);
        got.sort();
        assert_eq!(got, [(IN, r1.0 as u64), (IN, r2.0 as u64)]);
        // Delivered now: the one-shot item is disarmed and the edge has passed.
        assert_eq!(t.wait(ep, 0), []);
    }

    #[test]
    fn level_triggered_items_stay_ready_and_edge_triggered_ones_fire_once() {
        let t = Fixture::new();
        let (r1, w1) = pipe(&t.kernel, &t.caller(), OpenFlags::O_NONBLOCK);
        let (r2, w2) = pipe(&t.kernel, &t.caller(), OpenFlags::O_NONBLOCK);
        let ep = t.create();
        t.add(ep, r1, IN);
        t.add(ep, r2, IN | ET);
        assert_eq!(t.wait(ep, 0), []);

        t.write(w1, 4);
        t.write(w2, 4);
        assert_eq!(t.wait(ep, 0), [(IN, r1.0 as u64), (IN, r2.0 as u64)]);
        // A partial read leaves data: level-triggered reports again, edge-triggered does not.
        assert_eq!(t.read(r1, 1), Ok(1));
        assert_eq!(t.read(r2, 1), Ok(1));
        assert_eq!(t.wait(ep, 0), [(IN, r1.0 as u64)]);
        // New data is a new edge.
        t.write(w2, 1);
        assert_eq!(t.wait(ep, 0), [(IN, r1.0 as u64), (IN, r2.0 as u64)]);
        // Draining clears the level.
        assert_eq!(t.read(r1, 100), Ok(3));
        assert_eq!(t.wait(ep, 0), []);
        // The writer leaving is a hang-up, reported without being asked for.
        t.close(w1);
        assert_eq!(t.wait(ep, 0), [(HUP, r1.0 as u64)]);
    }

    #[test]
    fn oneshot_items_need_a_mod_to_rearm() {
        let t = Fixture::new();
        let (r, w) = pipe(&t.kernel, &t.caller(), OpenFlags::O_NONBLOCK);
        let ep = t.create();
        t.add(ep, r, IN | ONESHOT);
        t.write(w, 1);
        assert_eq!(t.wait(ep, 0).len(), 1);
        t.write(w, 1);
        assert_eq!(t.wait(ep, 0), []);

        assert_eq!(t.ctl(ep, EpollCtlOp::EPOLL_CTL_MOD, r, IN | ONESHOT), Ok(0));
        assert_eq!(t.wait(ep, 0), [(IN, r.0 as u64)]);
    }

    #[test]
    fn items_follow_the_open_file_not_the_descriptor() {
        let t = Fixture::new();
        let (r, w) = pipe(&t.kernel, &t.caller(), OpenFlags::O_NONBLOCK);
        let ep = t.create();
        let dup = t.dup(r);
        t.add(ep, r, IN);
        // The same file under another descriptor is another item.
        t.add(ep, dup, IN);
        t.write(w, 1);
        assert_eq!(t.wait(ep, 0).len(), 2);

        // Closing one descriptor keeps both items: the file is still open.
        t.close(r);
        let mut events = t.wait(ep, 0);
        events.sort();
        assert_eq!(events, [(IN, r.0 as u64), (IN, dup.0 as u64)]);
        assert_eq!(
            errno(t.ctl(ep, EpollCtlOp::EPOLL_CTL_DEL, r, 0)),
            Errno::EBADF
        );
        // Closing the last one removes them.
        t.close(dup);
        assert_eq!(t.wait(ep, 0), []);
    }

    #[test]
    fn ctl_errors_follow_linux() {
        let t = Fixture::new();
        let (r, _w) = pipe(&t.kernel, &t.caller(), OpenFlags::O_NONBLOCK);
        let ep = t.create();
        let ctl = |op, fd, events| errno(t.ctl(ep, op, fd, events));
        const ADD: EpollCtlOp = EpollCtlOp::EPOLL_CTL_ADD;
        const DEL: EpollCtlOp = EpollCtlOp::EPOLL_CTL_DEL;
        const MOD: EpollCtlOp = EpollCtlOp::EPOLL_CTL_MOD;

        assert_eq!(ctl(ADD, ep, IN), Errno::EINVAL);
        assert_eq!(ctl(ADD, Fd(50), IN), Errno::EBADF);
        assert_eq!(ctl(DEL, r, 0), Errno::ENOENT);
        assert_eq!(ctl(MOD, r, IN), Errno::ENOENT);
        assert_eq!(
            ctl(ADD, r, IN | EXCLUSIVE | EpollEvents::EPOLLPRI.0),
            Errno::EINVAL
        );
        assert_eq!(t.ctl(ep, ADD, r, IN | EXCLUSIVE), Ok(0));
        assert_eq!(ctl(ADD, r, IN), Errno::EEXIST);
        assert_eq!(ctl(MOD, r, IN), Errno::EINVAL);
        assert_eq!(ctl(EpollCtlOp(7), r, IN), Errno::EINVAL);
        // Not an epoll.
        assert_eq!(errno(t.ctl(r, ADD, ep, IN)), Errno::EINVAL);

        // Regular files cannot be watched.
        put(&t.memory, 0x700, b"/f\0");
        let open = OpenFlags::O_CREAT | OpenFlags::O_RDWR;
        let caller = t.caller();
        let file = block_on(
            t.kernel
                .openat(&caller, Fd(-100), GuestPtr::new(0x700), open, 0),
        );
        assert_eq!(ctl(ADD, file.unwrap(), IN), Errno::EPERM);
    }

    #[test]
    fn nested_epolls_report_readiness_and_refuse_loops() {
        let t = Fixture::new();
        let (r, w) = pipe(&t.kernel, &t.caller(), OpenFlags::O_NONBLOCK);
        let outer = t.create();
        let inner = t.create();
        t.add(inner, r, IN);
        t.add(outer, inner, IN);
        assert_eq!(t.wait(outer, 0), []);

        t.write(w, 1);
        assert_eq!(t.wait(outer, 0), [(IN, inner.0 as u64)]);
        assert_eq!(
            errno(t.ctl(inner, EpollCtlOp::EPOLL_CTL_ADD, outer, IN)),
            Errno::ELOOP
        );
        assert_eq!(
            errno(t.ctl(outer, EpollCtlOp::EPOLL_CTL_ADD, inner, IN | EXCLUSIVE)),
            Errno::EINVAL
        );

        // A chain deeper than EP_MAX_NESTS is refused.
        let mut chain = vec![t.create()];
        for _ in 0..4 {
            let next = t.create();
            t.add(next, *chain.last().unwrap(), IN);
            chain.push(next);
        }
        let top = t.create();
        let deep = t.ctl(top, EpollCtlOp::EPOLL_CTL_ADD, *chain.last().unwrap(), IN);
        assert_eq!(errno(deep), Errno::ELOOP);
    }

    /// `EPOLL_CTL_ADD` of `fd` with `EPOLLIN`, from a memory of its own, for other threads.
    fn add_from_thread(kernel: &Kernel, task: TaskId, epfd: Fd, fd: Fd) -> SysResult<i32> {
        let memory = FlatMemory(RefCell::new(vec![0; 64]));
        put(&memory, 0, &IN.to_le_bytes());
        let caller = Caller::new(task, &memory);
        let op = EpollCtlOp::EPOLL_CTL_ADD;
        block_on(kernel.epoll_ctl(&caller, epfd, op, fd, GuestPtr::new(0)))
    }

    #[test]
    fn concurrent_cross_adds_never_build_a_loop() {
        let t = Fixture::new();
        for _ in 0..200 {
            let (a, b) = (t.create(), t.create());
            let (ab, ba) = std::thread::scope(|scope| {
                let ab = scope.spawn(|| add_from_thread(&t.kernel, t.task, a, b));
                let ba = add_from_thread(&t.kernel, t.task, b, a);
                (ab.join().unwrap(), ba)
            });
            // Exactly one nesting wins; the other sees the loop.
            assert!(ab.is_ok() != ba.is_ok(), "{ab:?} {ba:?}");
            assert_eq!(ab.or(ba), Ok(0));
            t.close(a);
            t.close(b);
        }
    }

    #[test]
    fn closing_the_epoll_unregisters_its_items() {
        let t = Fixture::new();
        let (r, w) = pipe(&t.kernel, &t.caller(), OpenFlags::O_NONBLOCK);
        let ep = t.create();
        t.add(ep, r, IN | OUT);
        t.close(ep);
        // The pipe's queues hold no entries for the dead epoll; writing must not panic.
        t.write(w, 1);
        let file = t.kernel.file(t.task, r).unwrap();
        assert!(file.epoll_links.lock_or_recover().is_empty());
    }

    #[test]
    fn epoll_wait_blocks_until_another_thread_writes() {
        let t = Fixture::new();
        let (r, w) = pipe(&t.kernel, &t.caller(), OpenFlags(0));
        let ep = t.create();
        t.add(ep, r, IN);
        let writer = t.kernel.file(t.task, w).unwrap();

        std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(20));
                assert_eq!(
                    block_on(writer.write(&wasixv2_wait::NoInterrupt, &Default::default(), b"!")),
                    Ok(1)
                );
            });
            assert_eq!(t.wait(ep, -1), [(IN, r.0 as u64)]);
        });
        let bad = block_on(t.kernel.epoll_pwait(
            &t.caller(),
            ep,
            GuestPtrMut::new(0x900),
            0,
            0,
            GuestPtr::NULL,
            8,
        ));
        assert_eq!(errno(bad), Errno::EINVAL);
    }
}
