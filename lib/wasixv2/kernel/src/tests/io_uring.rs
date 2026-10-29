//! io_uring as liburing drives it: rings mapped from the ring file, SQEs written to them,
//! `io_uring_enter`, and CQEs read back from memory. On the AIO tests' host, with real timers
//! and shared memory objects.

use std::time::Instant;
use wasixv2_abi::{
    AddressFamily, EventfdFlags, FdPair, GuestLayout, IoUringCqe, IoUringOp, IoUringParams,
    IoUringSqe, IoringCqeFlags, IoringEnterFlags, IoringFeatFlags, IoringOpFlags, IoringSetupFlags,
    IoringSqFlags, IosqeFlags, KernelTimespec, MapFlags, OpenFlags, ProtFlags,
};

use wasixv2_wait::PollMask;

use super::{
    aio::Machine,
    ipc::{errno, pipe},
    *,
};

const PARAMS: u32 = 0x3000;
const BUF: u32 = 0x4000;
const TS: u32 = 0x3200;
const SCRATCH: u32 = 0x3400;

/// A ring as liburing maps it.
struct Ring<'a> {
    m: &'a Machine,
    fd: Fd,
    p: IoUringParams,
    rings: u32,
    sqes: u32,
}

impl Machine {
    fn setup_ring(&self, entries: u32, p: IoUringParams) -> SysResult<IoUringParams> {
        let mut bytes = [0; IoUringParams::SIZE];
        p.write_to(&mut bytes);
        self.memory.put(PARAMS, &bytes);
        let caller = self.caller();
        let fd = block_on(
            self.kernel
                .io_uring_setup(&caller, entries, GuestPtrMut::new(PARAMS)),
        )?;
        let p = IoUringParams::read_from(&self.memory.get(PARAMS, IoUringParams::SIZE)).unwrap();
        Ok(IoUringParams {
            wq_fd: fd as u32,
            ..p
        })
    }

    fn ring(&self, entries: u32, flags: u32) -> Ring<'_> {
        let p = IoUringParams {
            flags,
            ..IoUringParams::ZERO
        };
        let p = self.setup_ring(entries, p).unwrap();
        let fd = Fd(p.wq_fd as i32);
        let caller = self.caller();
        let map = |len: u32, offset: i64| {
            let flags = MapFlags::MAP_SHARED | MapFlags::MAP_POPULATE;
            let mmap = self
                .kernel
                .mmap(&caller, 0, len, ProtFlags(3), flags, fd, offset);
            block_on(mmap).unwrap()
        };
        let rings_len = (p.sq_off.array + p.sq_entries * 4).max(p.cq_off.cqes + p.cq_entries * 16);
        let rings = map(rings_len, 0);
        let sqes = map(p.sq_entries * 64, 0x1000_0000);
        Ring {
            m: self,
            fd,
            p,
            rings,
            sqes,
        }
    }

    fn put_word(&self, addr: u32, value: u32) {
        self.memory.put(addr, &value.to_le_bytes());
    }

    fn get_word(&self, addr: u32) -> u32 {
        u32::from_le_bytes(self.memory.get(addr, 4).try_into().unwrap())
    }
}

/// An SQE of `opcode` on `fd` with the other fields zeroed.
fn sqe(opcode: IoUringOp, fd: i32, user_data: u64) -> IoUringSqe {
    IoUringSqe {
        opcode: opcode.0,
        fd,
        user_data,
        ..IoUringSqe::ZERO
    }
}

impl Ring<'_> {
    /// Puts `sqe` on the SQ ring (`io_uring_get_sqe` and the array, then the tail).
    fn push(&self, sqe: IoUringSqe) {
        let off = &self.p.sq_off;
        let tail = self.m.get_word(self.rings + off.tail);
        let index = tail & (self.p.sq_entries - 1);
        let mut bytes = [0; IoUringSqe::SIZE];
        sqe.write_to(&mut bytes);
        self.m.memory.put(self.sqes + index * 64, &bytes);
        self.m.put_word(self.rings + off.array + index * 4, index);
        self.m.put_word(self.rings + off.tail, tail + 1);
    }

    fn enter(&self, to_submit: u32, min_complete: u32, flags: u32) -> SysResult<i32> {
        let caller = self.m.caller();
        let enter = self.m.kernel.io_uring_enter(
            &caller,
            self.fd,
            to_submit,
            min_complete,
            flags,
            GuestPtr::NULL,
            0,
        );
        block_on(enter)
    }

    fn submit(&self, n: u32) -> SysResult<i32> {
        self.enter(n, 0, 0)
    }

    fn submit_and_wait(&self, n: u32, wait: u32) -> SysResult<i32> {
        self.enter(n, wait, IoringEnterFlags::IORING_ENTER_GETEVENTS.0)
    }

    /// The CQEs on the ring, which it consumes.
    fn reap(&self) -> Vec<IoUringCqe> {
        let off = &self.p.cq_off;
        let head = self.m.get_word(self.rings + off.head);
        let tail = self.m.get_word(self.rings + off.tail);
        let mut cqes = Vec::new();
        for i in head..tail {
            let at = self.rings + off.cqes + (i & (self.p.cq_entries - 1)) * 16;
            cqes.push(IoUringCqe::read_from(&self.m.memory.get(at, 16)).unwrap());
        }
        self.m.put_word(self.rings + off.head, tail);
        cqes
    }

    /// Waits for `n` completions and takes them.
    fn wait(&self, n: u32) -> Vec<IoUringCqe> {
        assert_eq!(self.submit_and_wait(0, n), Ok(0));
        self.reap()
    }

    fn sq_flags(&self) -> u32 {
        self.m.get_word(self.rings + self.p.sq_off.flags)
    }

    fn register(&self, opcode: u32, arg: u32, nr: u32) -> SysResult<i32> {
        let caller = self.m.caller();
        let register =
            self.m
                .kernel
                .io_uring_register(&caller, self.fd, opcode, GuestPtrMut::new(arg), nr);
        block_on(register)
    }
}

fn res(cqes: &[IoUringCqe]) -> Vec<(u64, i32)> {
    cqes.iter().map(|cqe| (cqe.user_data, cqe.res)).collect()
}

fn write_ts(m: &Machine, at: u32, d: Duration) {
    let ts = KernelTimespec {
        tv_sec: d.as_secs() as i64,
        tv_nsec: i64::from(d.subsec_nanos()),
    };
    let mut bytes = [0; KernelTimespec::SIZE];
    ts.write_to(&mut bytes);
    m.memory.put(at, &bytes);
}

const NOP: IoUringOp = IoUringOp::IORING_OP_NOP;
const READ: IoUringOp = IoUringOp::IORING_OP_READ;
const WRITE: IoUringOp = IoUringOp::IORING_OP_WRITE;
const EINVAL: i32 = -Errno::EINVAL.0;

#[test]
fn setup_sizes_the_rings_and_refuses_polling_modes() {
    let m = Machine::new();
    let p = m.setup_ring(3, IoUringParams::ZERO).unwrap();
    assert_eq!((p.sq_entries, p.cq_entries), (4, 8));
    let single = IoringFeatFlags::IORING_FEAT_SINGLE_MMAP.0 | IoringFeatFlags::IORING_FEAT_NODROP.0;
    assert_eq!(p.features & single, single);
    assert_ne!(p.cq_off.cqes, 0);

    let sized = IoUringParams {
        flags: IoringSetupFlags::IORING_SETUP_CQSIZE.0,
        cq_entries: 100,
        ..IoUringParams::ZERO
    };
    let p = m.setup_ring(8, sized).unwrap();
    assert_eq!(p.cq_entries, 128);

    let flags = |flags: u32| IoUringParams {
        flags,
        ..IoUringParams::ZERO
    };
    for refused in [
        IoringSetupFlags::IORING_SETUP_SQPOLL.0,
        IoringSetupFlags::IORING_SETUP_IOPOLL.0,
        IoringSetupFlags::IORING_SETUP_DEFER_TASKRUN.0,
        1 << 30,
    ] {
        assert_eq!(errno(m.setup_ring(8, flags(refused))), Errno::EINVAL);
    }
    assert_eq!(errno(m.setup_ring(0, IoUringParams::ZERO)), Errno::EINVAL);
    assert_eq!(
        errno(m.setup_ring(65536, IoUringParams::ZERO)),
        Errno::EINVAL
    );
    let clamped = m
        .setup_ring(65536, flags(IoringSetupFlags::IORING_SETUP_CLAMP.0))
        .unwrap();
    assert_eq!(clamped.sq_entries, 32768);
    let resv = IoUringParams {
        resv: [0, 1, 0],
        ..IoUringParams::ZERO
    };
    assert_eq!(errno(m.setup_ring(8, resv)), Errno::EINVAL);
}

#[test]
fn nops_reads_and_writes_complete_on_the_cq_ring() {
    let m = Machine::new();
    let ring = m.ring(8, 0);
    let fd = m.file("data", b"hello world");
    ring.push(sqe(NOP, -1, 1));
    let inject = IoUringSqe {
        op_flags: IoringOpFlags::IORING_NOP_INJECT_RESULT.0,
        len: 42,
        ..sqe(NOP, -1, 2)
    };
    ring.push(inject);
    ring.push(IoUringSqe {
        addr: u64::from(BUF),
        len: 5,
        off: 6,
        ..sqe(READ, fd.0, 3)
    });
    m.memory.put(BUF + 0x100, b"HELLO");
    ring.push(IoUringSqe {
        addr: u64::from(BUF + 0x100),
        len: 5,
        off: 0,
        ..sqe(WRITE, fd.0, 4)
    });
    assert_eq!(ring.submit(4), Ok(4));
    let cqes = ring.reap();
    assert_eq!(res(&cqes), [(1, 0), (2, 42), (3, 5), (4, 5)]);
    assert_eq!(m.memory.get(BUF, 5), b"world");
    let caller = m.caller();
    let pread = m.kernel.pread64(&caller, fd, UserSliceMut::new(BUF, 11), 0);
    assert_eq!(block_on(pread), Ok(11));
    assert_eq!(m.memory.get(BUF, 11), b"HELLO world");

    // `-1` reads at the file position, which moves.
    let seek = m
        .kernel
        .lseek(&caller, fd, 0, wasixv2_abi::SeekWhence::SEEK_SET);
    assert_eq!(block_on(seek), Ok(0));
    ring.push(IoUringSqe {
        addr: u64::from(BUF),
        len: 3,
        off: u64::MAX,
        ..sqe(READ, fd.0, 5)
    });
    ring.push(IoUringSqe {
        addr: u64::from(BUF + 3),
        len: 3,
        off: u64::MAX,
        ..sqe(READ, fd.0, 6)
    });
    assert_eq!(ring.submit(2), Ok(2));
    assert_eq!(res(&ring.reap()), [(5, 3), (6, 3)]);
    assert_eq!(m.memory.get(BUF, 6), b"HELLO ");

    // An unknown opcode fails its request, which still counts as submitted.
    ring.push(IoUringSqe {
        opcode: 200,
        ..sqe(NOP, -1, 7)
    });
    ring.push(sqe(IoUringOp::IORING_OP_URING_CMD, -1, 8));
    ring.push(sqe(READ, 99, 9));
    assert_eq!(ring.submit(3), Ok(1));
    assert_eq!(ring.submit(2), Ok(1));
    assert_eq!(ring.submit(1), Ok(1));
    let failed = ring.reap();
    assert_eq!(
        res(&failed),
        [(7, EINVAL), (8, -Errno::EOPNOTSUPP.0), (9, -Errno::EBADF.0)]
    );
}

#[test]
fn a_read_that_would_block_completes_once_data_arrives() {
    let m = Machine::new();
    let ring = m.ring(8, 0);
    let (r, w) = pipe(&m.kernel, &m.caller(), OpenFlags(0));
    ring.push(IoUringSqe {
        addr: u64::from(BUF),
        len: 16,
        ..sqe(READ, r.0, 1)
    });
    assert_eq!(ring.submit(1), Ok(1));
    assert!(ring.reap().is_empty());
    let caller = m.caller();
    let poll = |kernel: &Kernel| {
        let file = kernel.file(caller.task(), ring.fd).unwrap();
        wasixv2_wait::Pollable::poll(&*file, None)
    };
    assert!(!poll(&m.kernel).contains(PollMask::READABLE));

    m.memory.put(BUF + 0x100, b"late");
    block_on(m.kernel.write(&caller, w, UserSlice::new(BUF + 0x100, 4))).unwrap();
    // The wakeup turns the read into task work, which the ring reports.
    let deadline = Instant::now() + Duration::from_secs(10);
    while !poll(&m.kernel).contains(PollMask::READABLE) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_ne!(ring.sq_flags() & IoringSqFlags::IORING_SQ_TASKRUN.0, 0);
    let cqes = ring.wait(1);
    assert_eq!(res(&cqes), [(1, 4)]);
    assert_eq!(m.memory.get(BUF, 4), b"late");
    assert_eq!(ring.sq_flags() & IoringSqFlags::IORING_SQ_TASKRUN.0, 0);

    // `RWF_NOWAIT` asks for `EAGAIN` instead, as `preadv2` gives it.
    ring.push(IoUringSqe {
        addr: u64::from(BUF),
        len: 16,
        op_flags: wasixv2_abi::RwfFlags::RWF_NOWAIT.0,
        ..sqe(READ, r.0, 2)
    });
    assert_eq!(ring.submit(1), Ok(1));
    assert_eq!(res(&ring.reap()), [(2, -Errno::EAGAIN.0)]);
    m.put_word(SCRATCH, BUF);
    m.put_word(SCRATCH + 4, 16);
    let flags = wasixv2_abi::RwfFlags::RWF_NOWAIT;
    let preadv2 = m
        .kernel
        .preadv2(&caller, r, GuestPtr::new(SCRATCH), 1, -1, flags);
    assert_eq!(errno(block_on(preadv2)), Errno::EAGAIN);
    // Nothing else of the ring file maps.
    let mmap = m.kernel.mmap(
        &caller,
        0,
        65536,
        ProtFlags(3),
        MapFlags::MAP_SHARED,
        ring.fd,
        0x2000_0000,
    );
    assert_eq!(errno(block_on(mmap)), Errno::EINVAL);
}

#[test]
fn links_run_in_order_and_a_failure_cancels_the_rest() {
    let m = Machine::new();
    let ring = m.ring(8, 0);
    let fd = m.file("l", b"");
    let link = IosqeFlags::IOSQE_IO_LINK.0;
    m.memory.put(BUF, b"abc");
    ring.push(IoUringSqe {
        flags: link,
        addr: u64::from(BUF),
        len: 3,
        ..sqe(WRITE, fd.0, 1)
    });
    ring.push(IoUringSqe {
        addr: u64::from(BUF + 0x100),
        len: 3,
        ..sqe(READ, fd.0, 2)
    });
    assert_eq!(ring.submit(2), Ok(2));
    assert_eq!(res(&ring.wait(2)), [(1, 3), (2, 3)]);
    assert_eq!(m.memory.get(BUF + 0x100, 3), b"abc");

    // A failing head cancels its link; a hard link goes on.
    ring.push(IoUringSqe {
        flags: link,
        ..sqe(READ, 99, 3)
    });
    ring.push(IoUringSqe {
        flags: link,
        ..sqe(NOP, -1, 4)
    });
    ring.push(sqe(NOP, -1, 5));
    ring.push(IoUringSqe {
        flags: IosqeFlags::IOSQE_IO_HARDLINK.0,
        ..sqe(READ, 99, 6)
    });
    ring.push(sqe(NOP, -1, 7));
    assert_eq!(ring.submit(5), Ok(5));
    let cqes = ring.wait(5);
    let canceled = -Errno::ECANCELED.0;
    let ebadf = -Errno::EBADF.0;
    assert_eq!(
        res(&cqes),
        [(3, ebadf), (4, canceled), (5, canceled), (6, ebadf), (7, 0)]
    );

    // `IOSQE_CQE_SKIP_SUCCESS` posts nothing for a success.
    ring.push(IoUringSqe {
        flags: IosqeFlags::IOSQE_CQE_SKIP_SUCCESS.0,
        ..sqe(NOP, -1, 8)
    });
    ring.push(sqe(NOP, -1, 9));
    assert_eq!(ring.submit(2), Ok(2));
    assert_eq!(res(&ring.reap()), [(9, 0)]);
}

#[test]
fn timeouts_fire_or_count_completions() {
    let m = Machine::new();
    let ring = m.ring(8, 0);
    write_ts(&m, TS, Duration::from_millis(20));
    let timeout = IoUringSqe {
        addr: u64::from(TS),
        len: 1,
        ..sqe(IoUringOp::IORING_OP_TIMEOUT, -1, 1)
    };
    ring.push(timeout);
    assert_eq!(ring.submit(1), Ok(1));
    let started = Instant::now();
    assert_eq!(res(&ring.wait(1)), [(1, -Errno::ETIME.0)]);
    assert!(started.elapsed() >= Duration::from_millis(10));

    // Ends after two completions, with 0.
    write_ts(&m, TS, Duration::from_secs(30));
    ring.push(IoUringSqe { off: 2, ..timeout });
    ring.push(sqe(NOP, -1, 2));
    ring.push(sqe(NOP, -1, 3));
    assert_eq!(ring.submit(3), Ok(3));
    assert_eq!(res(&ring.wait(3)), [(2, 0), (3, 0), (1, 0)]);

    // Removed: ECANCELED for it, 0 for the removal; nothing left: ENOENT.
    ring.push(timeout);
    assert_eq!(ring.submit(1), Ok(1));
    let remove = IoUringSqe {
        addr: 1,
        ..sqe(IoUringOp::IORING_OP_TIMEOUT_REMOVE, -1, 9)
    };
    ring.push(remove);
    assert_eq!(ring.submit(1), Ok(1));
    assert_eq!(res(&ring.reap()), [(1, -Errno::ECANCELED.0), (9, 0)]);
    ring.push(remove);
    assert_eq!(ring.submit(1), Ok(1));
    assert_eq!(res(&ring.reap()), [(9, -Errno::ENOENT.0)]);

    // A timeout that waits: io_uring_enter with an extended argument times out with ETIME.
    write_ts(&m, TS, Duration::from_millis(10));
    let arg = wasixv2_abi::IoUringGeteventsArg {
        sigmask: 0,
        sigmask_sz: 0,
        min_wait_usec: 0,
        ts: u64::from(TS),
    };
    let mut bytes = [0; wasixv2_abi::IoUringGeteventsArg::SIZE];
    arg.write_to(&mut bytes);
    m.memory.put(SCRATCH, &bytes);
    let caller = m.caller();
    let flags =
        IoringEnterFlags::IORING_ENTER_GETEVENTS.0 | IoringEnterFlags::IORING_ENTER_EXT_ARG.0;
    let size = wasixv2_abi::IoUringGeteventsArg::SIZE as u32;
    let enter =
        m.kernel
            .io_uring_enter(&caller, ring.fd, 0, 1, flags, GuestPtr::new(SCRATCH), size);
    assert_eq!(errno(block_on(enter)), Errno::ETIME);
}

#[test]
fn a_link_timeout_cancels_the_request_it_guards() {
    let m = Machine::new();
    let ring = m.ring(8, 0);
    let (r, _w) = pipe(&m.kernel, &m.caller(), OpenFlags(0));
    write_ts(&m, TS, Duration::from_millis(10));
    ring.push(IoUringSqe {
        flags: IosqeFlags::IOSQE_IO_LINK.0,
        addr: u64::from(BUF),
        len: 8,
        ..sqe(READ, r.0, 1)
    });
    ring.push(IoUringSqe {
        addr: u64::from(TS),
        len: 1,
        ..sqe(IoUringOp::IORING_OP_LINK_TIMEOUT, -1, 2)
    });
    assert_eq!(ring.submit(2), Ok(2));
    let mut cqes = ring.wait(2);
    cqes.sort_by_key(|cqe| cqe.user_data);
    assert_eq!(res(&cqes), [(1, -Errno::ECANCELED.0), (2, -Errno::ETIME.0)]);

    // A request that completes first cancels its link timeout.
    let fd = m.file("lt", b"x");
    ring.push(IoUringSqe {
        flags: IosqeFlags::IOSQE_IO_LINK.0,
        addr: u64::from(BUF),
        len: 1,
        ..sqe(READ, fd.0, 3)
    });
    ring.push(IoUringSqe {
        addr: u64::from(TS),
        len: 1,
        ..sqe(IoUringOp::IORING_OP_LINK_TIMEOUT, -1, 4)
    });
    assert_eq!(ring.submit(2), Ok(2));
    assert_eq!(res(&ring.wait(2)), [(4, -Errno::ECANCELED.0), (3, 1)]);

    // Alone, a link timeout has nothing to guard.
    ring.push(IoUringSqe {
        addr: u64::from(TS),
        len: 1,
        ..sqe(IoUringOp::IORING_OP_LINK_TIMEOUT, -1, 5)
    });
    assert_eq!(ring.submit(1), Ok(1));
    assert_eq!(res(&ring.reap()), [(5, EINVAL)]);
}

#[test]
fn polls_complete_when_ready_and_can_be_removed_or_cancelled() {
    let m = Machine::new();
    let ring = m.ring(8, 0);
    let (r, w) = pipe(&m.kernel, &m.caller(), OpenFlags(0));
    let poll = IoUringSqe {
        op_flags: PollMask::READABLE.0,
        ..sqe(IoUringOp::IORING_OP_POLL_ADD, r.0, 1)
    };
    ring.push(poll);
    assert_eq!(ring.submit(1), Ok(1));
    assert!(ring.reap().is_empty());
    m.memory.put(BUF, b"!");
    let caller = m.caller();
    block_on(m.kernel.write(&caller, w, UserSlice::new(BUF, 1))).unwrap();
    let cqes = ring.wait(1);
    assert_eq!(cqes[0].user_data, 1);
    assert_ne!(cqes[0].res as u32 & PollMask::READABLE.0, 0);

    // A ready file completes the poll at once; a multishot poll says more will come.
    let multi = IoUringSqe {
        len: IoringOpFlags::IORING_POLL_ADD_MULTI.0,
        ..poll
    };
    ring.push(IoUringSqe {
        user_data: 2,
        ..multi
    });
    assert_eq!(ring.submit(1), Ok(1));
    let cqes = ring.wait(1);
    assert_eq!(
        cqes[0].flags & IoringCqeFlags::IORING_CQE_F_MORE.0,
        IoringCqeFlags::IORING_CQE_F_MORE.0
    );
    // Removing it ends it with ECANCELED.
    ring.push(IoUringSqe {
        addr: 2,
        ..sqe(IoUringOp::IORING_OP_POLL_REMOVE, -1, 3)
    });
    assert_eq!(ring.submit(1), Ok(1));
    let mut cqes = ring.wait(2);
    cqes.retain(|cqe| cqe.flags & IoringCqeFlags::IORING_CQE_F_MORE.0 == 0);
    cqes.sort_by_key(|cqe| cqe.user_data);
    assert_eq!(res(&cqes), [(2, -Errno::ECANCELED.0), (3, 0)]);
}

#[test]
fn async_cancel_finds_waiting_requests() {
    let m = Machine::new();
    let ring = m.ring(8, 0);
    let (r, _w) = pipe(&m.kernel, &m.caller(), OpenFlags(0));
    for user_data in [1, 2] {
        ring.push(IoUringSqe {
            addr: u64::from(BUF),
            len: 1,
            ..sqe(READ, r.0, user_data)
        });
    }
    assert_eq!(ring.submit(2), Ok(2));
    let cancel = sqe(IoUringOp::IORING_OP_ASYNC_CANCEL, -1, 9);
    ring.push(IoUringSqe { addr: 1, ..cancel });
    assert_eq!(ring.submit(1), Ok(1));
    assert_eq!(res(&ring.reap()), [(1, -Errno::ECANCELED.0), (9, 0)]);
    ring.push(IoUringSqe { addr: 1, ..cancel });
    assert_eq!(ring.submit(1), Ok(1));
    assert_eq!(res(&ring.reap()), [(9, -Errno::ENOENT.0)]);
    // By descriptor, every one.
    let all = IoringOpFlags::IORING_ASYNC_CANCEL_ALL.0 | IoringOpFlags::IORING_ASYNC_CANCEL_FD.0;
    ring.push(IoUringSqe {
        op_flags: all,
        fd: r.0,
        ..cancel
    });
    assert_eq!(ring.submit(1), Ok(1));
    assert_eq!(res(&ring.reap()), [(2, -Errno::ECANCELED.0), (9, 1)]);
}

#[test]
fn a_drain_waits_for_what_was_submitted_before_it() {
    let m = Machine::new();
    let ring = m.ring(8, 0);
    let (r, w) = pipe(&m.kernel, &m.caller(), OpenFlags(0));
    ring.push(IoUringSqe {
        addr: u64::from(BUF),
        len: 1,
        ..sqe(READ, r.0, 1)
    });
    ring.push(IoUringSqe {
        flags: IosqeFlags::IOSQE_IO_DRAIN.0,
        ..sqe(NOP, -1, 2)
    });
    ring.push(sqe(NOP, -1, 3));
    assert_eq!(ring.submit(3), Ok(3));
    assert!(ring.reap().is_empty());
    m.memory.put(BUF + 8, b"d");
    let caller = m.caller();
    block_on(m.kernel.write(&caller, w, UserSlice::new(BUF + 8, 1))).unwrap();
    assert_eq!(res(&ring.wait(3)), [(1, 1), (2, 0), (3, 0)]);
}

#[test]
fn registered_files_buffers_and_eventfd() {
    let m = Machine::new();
    let ring = m.ring(8, 0);
    let fd = m.file("r", b"registered");
    // Slot 1 holds the file, slot 0 is empty.
    m.memory.put(
        SCRATCH,
        &[(-1i32).to_le_bytes(), fd.0.to_le_bytes()].concat(),
    );
    assert_eq!(ring.register(2, SCRATCH, 2), Ok(0));
    assert_eq!(errno(ring.register(2, SCRATCH, 2)), Errno::EBUSY);
    let fixed = IosqeFlags::IOSQE_FIXED_FILE.0;
    ring.push(IoUringSqe {
        flags: fixed,
        addr: u64::from(BUF),
        len: 4,
        ..sqe(READ, 1, 1)
    });
    ring.push(IoUringSqe {
        flags: fixed,
        addr: u64::from(BUF),
        len: 4,
        ..sqe(READ, 0, 2)
    });
    assert_eq!(ring.submit(2), Ok(2));
    assert_eq!(res(&ring.reap()), [(1, 4), (2, -Errno::EBADF.0)]);

    // A registered buffer bounds READ_FIXED.
    let iovec: Vec<u8> = [BUF + 0x100, 16]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    m.memory.put(SCRATCH, &iovec);
    assert_eq!(ring.register(0, SCRATCH, 1), Ok(0));
    let read_fixed = sqe(IoUringOp::IORING_OP_READ_FIXED, fd.0, 3);
    ring.push(IoUringSqe {
        addr: u64::from(BUF + 0x104),
        len: 8,
        ..read_fixed
    });
    ring.push(IoUringSqe {
        addr: u64::from(BUF + 0x10c),
        len: 8,
        ..read_fixed
    });
    assert_eq!(ring.submit(2), Ok(2));
    assert_eq!(res(&ring.reap()), [(3, 8), (3, -Errno::EFAULT.0)]);
    assert_eq!(m.memory.get(BUF + 0x104, 8), b"register");

    // A registered eventfd counts completions.
    let caller = m.caller();
    let efd = block_on(m.kernel.eventfd2(&caller, 0, EventfdFlags::EFD_NONBLOCK)).unwrap();
    m.memory.put(SCRATCH, &efd.0.to_le_bytes());
    assert_eq!(ring.register(4, SCRATCH, 1), Ok(0));
    ring.push(sqe(NOP, -1, 4));
    ring.push(sqe(NOP, -1, 5));
    assert_eq!(ring.submit(2), Ok(2));
    let count = m.kernel.read(&caller, efd, UserSliceMut::new(SCRATCH, 8));
    assert_eq!(block_on(count), Ok(8));
    assert_eq!(m.memory.get(SCRATCH, 8), 2u64.to_le_bytes());
    assert_eq!(ring.register(5, 0, 0), Ok(0));
    assert_eq!(errno(ring.register(5, 0, 0)), Errno::ENXIO);
}

#[test]
fn provided_buffers_are_selected_and_reported_in_the_cqe() {
    let m = Machine::new();
    let ring = m.ring(8, 0);
    let fd = m.file("p", b"0123456789");
    ring.push(IoUringSqe {
        fd: 2,
        addr: u64::from(BUF),
        len: 4,
        off: 7,
        buf_index: 1,
        ..sqe(IoUringOp::IORING_OP_PROVIDE_BUFFERS, 2, 1)
    });
    assert_eq!(ring.submit(1), Ok(1));
    assert_eq!(res(&ring.reap()), [(1, 0)]);
    let select = IoUringSqe {
        flags: IosqeFlags::IOSQE_BUFFER_SELECT.0,
        len: 4,
        buf_index: 1,
        ..sqe(READ, fd.0, 2)
    };
    for _ in 0..3 {
        ring.push(select);
    }
    assert_eq!(ring.submit(3), Ok(3));
    let cqes = ring.reap();
    let buffer = IoringCqeFlags::IORING_CQE_F_BUFFER.0;
    assert_eq!((cqes[0].res, cqes[0].flags), (4, buffer | 7 << 16));
    assert_eq!((cqes[1].res, cqes[1].flags), (4, buffer | 8 << 16));
    assert_eq!(cqes[2].res, -Errno::ENOBUFS.0);
    assert_eq!(m.memory.get(BUF, 8), b"01230123");

    ring.push(IoUringSqe {
        fd: 5,
        buf_index: 1,
        ..sqe(IoUringOp::IORING_OP_REMOVE_BUFFERS, 5, 3)
    });
    assert_eq!(ring.submit(1), Ok(1));
    assert_eq!(res(&ring.reap()), [(3, 0)]);
}

#[test]
fn the_probe_lists_the_supported_operations() {
    let m = Machine::new();
    let ring = m.ring(4, 0);
    let size = 16 + 70 * 8;
    m.memory.put(SCRATCH, &vec![0; size]);
    assert_eq!(ring.register(8, SCRATCH, 70), Ok(0));
    let probe = m.memory.get(SCRATCH, size);
    let last = IoUringOp::IORING_OP_LAST.0;
    assert_eq!((probe[0], probe[1]), (last - 1, last));
    let supported = |op: IoUringOp| probe[16 + usize::from(op.0) * 8 + 2] & 1 == 1;
    assert!(supported(READ) && supported(IoUringOp::IORING_OP_ACCEPT));
    assert!(!supported(IoUringOp::IORING_OP_SPLICE));
    // The buffer must come zeroed.
    m.memory.put(SCRATCH + 20, &[1]);
    assert_eq!(errno(ring.register(8, SCRATCH, 70)), Errno::EINVAL);
}

#[test]
fn file_operations_run_in_the_callers_table() {
    let m = Machine::new();
    let ring = m.ring(8, 0);
    m.memory.put(SCRATCH, b"opened\0");
    ring.push(IoUringSqe {
        addr: u64::from(SCRATCH),
        op_flags: (OpenFlags::O_RDWR | OpenFlags::O_CREAT).0,
        len: 0o600,
        ..sqe(IoUringOp::IORING_OP_OPENAT, -100, 1)
    });
    assert_eq!(ring.submit(1), Ok(1));
    let fd = ring.reap()[0].res;
    assert!(fd >= 0, "{fd}");
    ring.push(IoUringSqe {
        off: 0,
        addr: 4096,
        len: 0,
        ..sqe(IoUringOp::IORING_OP_FALLOCATE, fd, 2)
    });
    ring.push(IoUringSqe {
        addr: u64::from(SCRATCH),
        off: u64::from(BUF),
        len: 0x7ff,
        ..sqe(IoUringOp::IORING_OP_STATX, -100, 3)
    });
    ring.push(sqe(IoUringOp::IORING_OP_CLOSE, fd, 4));
    ring.push(sqe(IoUringOp::IORING_OP_CLOSE, fd, 5));
    assert_eq!(ring.submit(4), Ok(4));
    assert_eq!(
        res(&ring.reap()),
        [(2, 0), (3, 0), (4, 0), (5, -Errno::EBADF.0)]
    );
    // `stx_size`.
    assert_eq!(m.memory.get(BUF + 40, 8), 4096u64.to_le_bytes());
    // A ring cannot close itself this way.
    ring.push(sqe(IoUringOp::IORING_OP_CLOSE, ring.fd.0, 6));
    assert_eq!(ring.submit(1), Ok(1));
    assert_eq!(res(&ring.reap()), [(6, -Errno::EBADF.0)]);
}

#[test]
fn sockets_send_receive_accept_and_connect() {
    let m = Machine::new();
    let ring = m.ring(8, 0);
    let caller = m.caller();
    let pair =
        block_on(
            m.kernel
                .socketpair(&caller, AddressFamily::AF_UNIX, 1, 0, FdPair::new(SCRATCH)),
        );
    assert_eq!(pair, Ok(0));
    let (a, b) = (m.get_word(SCRATCH) as i32, m.get_word(SCRATCH + 4) as i32);
    ring.push(IoUringSqe {
        addr: u64::from(BUF),
        len: 16,
        ..sqe(IoUringOp::IORING_OP_RECV, b, 1)
    });
    assert_eq!(ring.submit(1), Ok(1));
    assert!(ring.reap().is_empty());
    m.memory.put(BUF + 0x100, b"ping");
    ring.push(IoUringSqe {
        addr: u64::from(BUF + 0x100),
        len: 4,
        ..sqe(IoUringOp::IORING_OP_SEND, a, 2)
    });
    assert_eq!(ring.submit(1), Ok(1));
    let mut cqes = ring.wait(2);
    cqes.sort_by_key(|cqe| cqe.user_data);
    assert_eq!(res(&cqes), [(1, 4), (2, 4)]);
    assert_eq!(m.memory.get(BUF, 4), b"ping");

    // An abstract Unix address to listen on.
    let addr: Vec<u8> = [1u16.to_le_bytes().to_vec(), b"\0uring".to_vec()].concat();
    m.memory.put(SCRATCH + 0x100, &addr);
    let listener = block_on(m.kernel.socket(&caller, AddressFamily::AF_UNIX, 1, 0)).unwrap();
    let bind = m.kernel.bind(
        &caller,
        listener,
        GuestPtr::new(SCRATCH + 0x100),
        addr.len() as u32,
    );
    assert_eq!(block_on(bind), Ok(0));
    assert_eq!(block_on(m.kernel.listen(&caller, listener, 4)), Ok(0));
    ring.push(sqe(IoUringOp::IORING_OP_ACCEPT, listener.0, 3));
    assert_eq!(ring.submit(1), Ok(1));
    assert!(ring.reap().is_empty());
    let client = block_on(m.kernel.socket(&caller, AddressFamily::AF_UNIX, 1, 0)).unwrap();
    ring.push(IoUringSqe {
        addr: u64::from(SCRATCH + 0x100),
        off: addr.len() as u64,
        ..sqe(IoUringOp::IORING_OP_CONNECT, client.0, 4)
    });
    assert_eq!(ring.submit(1), Ok(1));
    let mut cqes = ring.wait(2);
    cqes.sort_by_key(|cqe| cqe.user_data);
    assert_eq!(cqes[0].user_data, 3);
    assert!(cqes[0].res >= 0, "{:?}", res(&cqes));
    assert_eq!((cqes[1].user_data, cqes[1].res), (4, 0));
}

#[test]
fn fdinfo_shows_the_rings() {
    let m = Machine::new();
    let ring = m.ring(4, 0);
    let caller = m.caller();
    let file = m.kernel.file(caller.task(), ring.fd).unwrap();
    let info = file.object().unwrap().show_fdinfo();
    assert!(info.contains("SqMask:\t0x3\n"), "{info}");
    assert!(info.contains("CqMask:\t0x7\n"), "{info}");
    assert!(info.contains("UserFiles:\t0\n"), "{info}");
}
