//! Linux AIO: the ring `io_setup` maps, requests that complete in `io_submit`, polls that
//! complete from the timer driver, cancellation and the end of a context. On a host with real
//! threads and shared memory objects, over memory that shows them.

use std::time::Instant;

use futures::FutureExt as _;

use wasixv2_abi::{
    AioRingMisc, EventfdFlags, GuestLayout, IoEvent, Iocb, IocbCmd, IocbFlags, KernelTimespec,
    MapFlags, MremapFlags, OpenFlags, ProtFlags,
};
use wasixv2_pages::{Memfd, SharedPages};
use wasixv2_wait::PollMask;

use super::{
    ipc::{errno, pipe},
    mapped::MappedMemory,
    ptimer::RealHost,
    *,
};

const CTXP: u32 = 0x1000;
const IOCB: u32 = 0x1100;
const IOCBS: u32 = 0x1300;
const EVENTS: u32 = 0x1400;
const TIMEOUT: u32 = 0x1600;
const PATH: u32 = 0x1700;
const BUF: u32 = 0x2000;
const IOVECS: u32 = 0x1680;

/// Real clocks and threads, so the timer driver runs, and shared memory objects for rings.
struct AioHost(RealHost);

impl Host for AioHost {
    fn realtime(&self) -> Duration {
        self.0.realtime()
    }

    fn monotonic(&self) -> Duration {
        self.0.monotonic()
    }

    fn sleep(&self, duration: Duration) -> LocalFuture<'_, ()> {
        self.0.sleep(duration)
    }

    fn random(&self, buf: &mut [u8]) {
        self.0.random(buf)
    }

    fn executor(&self) -> &dyn Executor {
        self.0.executor()
    }

    fn shared_pages(&self, size: u64) -> io::Result<Arc<dyn SharedPages>> {
        Ok(Arc::new(Memfd::new(size)?))
    }
}

pub(super) struct Machine {
    pub(super) kernel: Kernel,
    pub(super) task: TaskId,
    pub(super) memory: MappedMemory,
}

impl Machine {
    pub(super) fn new() -> Self {
        Self::with_credentials(Credentials::default())
    }

    pub(super) fn with_credentials(credentials: Credentials) -> Self {
        Self::configured(|kernel| kernel.with_credentials(credentials))
    }

    pub(super) fn configured(configure: impl FnOnce(Kernel) -> Kernel) -> Self {
        Self::on_host(Arc::new(AioHost(RealHost(Instant::now()))), configure)
    }

    pub(super) fn on_host(host: Arc<dyn Host>, configure: impl FnOnce(Kernel) -> Kernel) -> Self {
        let root = MemFs::new(Arc::new(HostClock(host.clone())), 0o755, 1, 1);
        let kernel = configure(Kernel::new(host, Arc::new(root)));
        let stdin = Arc::new(Input(Mutex::new(Vec::new())));
        let (kernel, task, _) = spawn_on(kernel, stdin, &[], &[]);
        Self {
            kernel,
            task,
            memory: MappedMemory::new(2, false),
        }
    }

    pub(super) fn caller(&self) -> Caller<'_> {
        Caller::new(self.task, &self.memory)
    }

    /// A regular file at `name` holding `data`, open for reading and writing.
    pub(super) fn file(&self, name: &str, data: &[u8]) -> Fd {
        let mut path = name.as_bytes().to_vec();
        path.push(0);
        self.memory.put(PATH, &path);
        let flags = OpenFlags::O_RDWR | OpenFlags::O_CREAT;
        let caller = self.caller();
        let open = self
            .kernel
            .openat(&caller, Fd(-100), GuestPtr::new(PATH), flags, 0o600);
        let fd = block_on(open).unwrap();
        self.memory.put(BUF, data);
        let buf = UserSlice::new(BUF, data.len() as u32);
        block_on(self.kernel.write(&self.caller(), fd, buf)).unwrap();
        fd
    }

    fn setup(&self, nr: u32) -> SysResult<i32> {
        self.memory.put(CTXP, &0u32.to_le_bytes());
        block_on(
            self.kernel
                .io_setup(&self.caller(), nr, GuestPtrMut::new(CTXP)),
        )
    }

    fn ctx(&self) -> u32 {
        self.setup(8).unwrap();
        self.word(CTXP)
    }

    fn word(&self, addr: u32) -> u32 {
        u32::from_le_bytes(self.memory.get(addr, 4).try_into().unwrap())
    }

    /// Writes an iocb at `at`.
    fn iocb(&self, at: u32, cmd: IocbCmd, fd: Fd, (buf, nbytes, offset): (u32, u64, i64)) {
        let iocb = Iocb {
            aio_data: u64::from(at) << 8,
            aio_key: 0xffff,
            aio_rw_flags: 0,
            aio_lio_opcode: cmd.0,
            aio_reqprio: 0,
            aio_fildes: fd.0 as u32,
            aio_buf: buf.into(),
            aio_nbytes: nbytes,
            aio_offset: offset,
            aio_reserved2: 0,
            aio_flags: 0,
            aio_resfd: 0,
        };
        self.put_iocb(at, &iocb);
    }

    fn put_iocb(&self, at: u32, iocb: &Iocb) {
        let mut bytes = [0; Iocb::SIZE];
        iocb.write_to(&mut bytes);
        self.memory.put(at, &bytes);
    }

    fn read_iocb(&self, at: u32) -> Iocb {
        Iocb::read_from(&self.memory.get(at, Iocb::SIZE)).unwrap()
    }

    /// `io_submit` of the iocbs at `iocbs`.
    fn submit(&self, ctx: u32, iocbs: &[u32]) -> SysResult<i32> {
        let list: Vec<u8> = iocbs.iter().flat_map(|a| a.to_le_bytes()).collect();
        self.memory.put(IOCBS, &list);
        let nr = iocbs.len() as i32;
        block_on(
            self.kernel
                .io_submit(&self.caller(), ctx, nr, GuestPtr::new(IOCBS)),
        )
    }

    /// `io_getevents` waiting at most `timeout`, or forever.
    fn getevents(&self, ctx: u32, min: i32, nr: i32, timeout: Option<Duration>) -> Vec<IoEvent> {
        let ts = match timeout {
            Some(timeout) => {
                let ts = KernelTimespec {
                    tv_sec: timeout.as_secs() as i64,
                    tv_nsec: i64::from(timeout.subsec_nanos()),
                };
                let mut bytes = [0; KernelTimespec::SIZE];
                ts.write_to(&mut bytes);
                self.memory.put(TIMEOUT, &bytes);
                GuestPtrMut::new(TIMEOUT)
            }
            None => GuestPtrMut::NULL,
        };
        let events = GuestPtrMut::new(EVENTS);
        let caller = self.caller();
        let got =
            self.kernel
                .io_pgetevents_time64(&caller, ctx, min, nr, events, ts, GuestPtr::NULL);
        let got = block_on(got).unwrap() as usize;
        let bytes = self.memory.get(EVENTS, got * IoEvent::SIZE);
        bytes
            .chunks_exact(IoEvent::SIZE)
            .map(|event| IoEvent::read_from(event).unwrap())
            .collect()
    }
}

#[test]
fn io_setup_maps_a_ring_libaio_can_read() {
    let m = Machine::new();
    let ctx = m.ctx();
    assert_ne!(ctx, 0);
    // `struct aio_ring`: id, nr, head, tail, magic, compat, incompat, header_length.
    assert_eq!(m.word(ctx), 0);
    let nr = m.word(ctx + 4);
    assert!(nr >= 16, "{nr}");
    assert_eq!((m.word(ctx + 8), m.word(ctx + 12)), (0, 0));
    assert_eq!(m.word(ctx + 16), AioRingMisc::AIO_RING_MAGIC.0);
    assert_eq!(m.word(ctx + 20), 1);
    assert_eq!(m.word(ctx + 24), 0);
    assert_eq!(m.word(ctx + 28), 32);
    assert_eq!(m.kernel.aio.nr(), 8);

    // A second context is a second ring, with the next id.
    let other = m.ctx();
    assert_ne!(other, ctx);
    assert_eq!(m.word(other), 1);
    assert_eq!(m.kernel.aio.nr(), 16);
}

#[test]
fn io_setup_checks_its_arguments_and_the_limit() {
    let m = Machine::new();
    assert_eq!(errno(m.setup(0)), Errno::EINVAL);
    m.memory.put(CTXP, &1u32.to_le_bytes());
    let caller = m.caller();
    let setup = m.kernel.io_setup(&caller, 8, GuestPtrMut::new(CTXP));
    assert_eq!(errno(block_on(setup)), Errno::EINVAL);
    m.kernel.aio.set_max(10);
    assert_eq!(errno(m.setup(11)), Errno::EAGAIN);
    m.setup(6).unwrap();
    assert_eq!(errno(m.setup(6)), Errno::EAGAIN);
    assert_eq!(m.kernel.aio.nr(), 6);
}

#[test]
fn reads_and_writes_complete_in_io_submit_and_land_on_the_ring() {
    let m = Machine::new();
    let ctx = m.ctx();
    let fd = m.file("data", b"0123456789");
    let (read, write) = (IOCB, IOCB + 0x40);
    m.iocb(read, IocbCmd::IOCB_CMD_PREAD, fd, (BUF + 0x100, 4, 3));
    m.memory.put(BUF + 0x200, b"ab");
    m.iocb(write, IocbCmd::IOCB_CMD_PWRITE, fd, (BUF + 0x200, 2, 8));
    assert_eq!(m.submit(ctx, &[read, write]), Ok(2));
    // `io_submit` writes `KIOCB_KEY`.
    assert_eq!(m.read_iocb(read).aio_key, 0);
    assert_eq!(m.memory.get(BUF + 0x100, 4), b"3456");
    // Both events are on the ring before anybody asks.
    assert_eq!(m.word(ctx + 12), 2);

    let events = m.getevents(ctx, 2, 8, None);
    assert_eq!(events.len(), 2);
    assert_eq!(
        (events[0].obj, events[0].data, events[0].res),
        (read.into(), u64::from(read) << 8, 4)
    );
    assert_eq!((events[1].obj, events[1].res), (write.into(), 2));
    assert_eq!(m.word(ctx + 8), 2, "the head moved past them");

    let caller = m.caller();
    let pread = m.kernel.pread64(&caller, fd, UserSliceMut::new(BUF, 10), 0);
    assert_eq!(block_on(pread), Ok(10));
    assert_eq!(m.memory.get(BUF, 10), b"01234567ab");
}

#[test]
fn vectored_requests_take_an_iovec_array() {
    let m = Machine::new();
    let ctx = m.ctx();
    let fd = m.file("v", b"hello world");
    let iovecs: Vec<u8> = [BUF + 0x100, 5, BUF + 0x180, 6]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    m.memory.put(IOVECS, &iovecs);
    m.iocb(IOCB, IocbCmd::IOCB_CMD_PREADV, fd, (IOVECS, 2, 0));
    assert_eq!(m.submit(ctx, &[IOCB]), Ok(1));
    let events = m.getevents(ctx, 1, 1, None);
    assert_eq!(events[0].res, 11);
    assert_eq!(m.memory.get(BUF + 0x100, 5), b"hello");
    assert_eq!(m.memory.get(BUF + 0x180, 6), b" world");
}

#[test]
fn a_bad_iocb_stops_io_submit_after_the_good_ones() {
    let m = Machine::new();
    let ctx = m.ctx();
    let fd = m.file("f", b"x");
    let (good, bad) = (IOCB, IOCB + 0x40);
    m.iocb(good, IocbCmd::IOCB_CMD_PREAD, fd, (BUF, 1, 0));
    m.iocb(bad, IocbCmd::IOCB_CMD_PREAD, Fd(99), (BUF, 1, 0));
    assert_eq!(m.submit(ctx, &[good, bad]), Ok(1));
    assert_eq!(errno(m.submit(ctx, &[bad])), Errno::EBADF);

    m.iocb(bad, IocbCmd::IOCB_CMD_NOOP, fd, (0, 0, 0));
    assert_eq!(errno(m.submit(ctx, &[bad])), Errno::EINVAL);
    let mut reserved = m.read_iocb(good);
    reserved.aio_reserved2 = 1;
    m.put_iocb(bad, &reserved);
    assert_eq!(errno(m.submit(ctx, &[bad])), Errno::EINVAL);
    m.iocb(bad, IocbCmd::IOCB_CMD_PREAD, fd, (BUF, 1, -1));
    assert_eq!(errno(m.submit(ctx, &[bad])), Errno::EINVAL);
    assert_eq!(errno(m.submit(0x1234, &[good])), Errno::EINVAL);
    // Only the first one made an event.
    assert_eq!(m.getevents(ctx, 0, 8, Some(Duration::ZERO)).len(), 1);
}

#[test]
fn fsync_needs_a_file_and_empty_fields() {
    let m = Machine::new();
    let ctx = m.ctx();
    let fd = m.file("s", b"");
    let (r, _w) = pipe(&m.kernel, &m.caller(), OpenFlags(0));
    m.iocb(IOCB, IocbCmd::IOCB_CMD_FSYNC, r, (0, 0, 0));
    assert_eq!(errno(m.submit(ctx, &[IOCB])), Errno::EINVAL);
    m.iocb(IOCB, IocbCmd::IOCB_CMD_FDSYNC, fd, (0, 1, 0));
    assert_eq!(errno(m.submit(ctx, &[IOCB])), Errno::EINVAL);
    m.iocb(IOCB, IocbCmd::IOCB_CMD_FDSYNC, fd, (0, 0, 0));
    assert_eq!(m.submit(ctx, &[IOCB]), Ok(1));
    assert_eq!(m.getevents(ctx, 1, 1, None)[0].res, 0);
}

#[test]
fn a_poll_completes_once_the_file_is_ready_and_signals_its_eventfd() {
    let m = Machine::new();
    let ctx = m.ctx();
    let (r, w) = pipe(&m.kernel, &m.caller(), OpenFlags(0));
    let efd = block_on(
        m.kernel
            .eventfd2(&m.caller(), 0, EventfdFlags::EFD_NONBLOCK),
    )
    .unwrap();
    m.iocb(
        IOCB,
        IocbCmd::IOCB_CMD_POLL,
        r,
        (PollMask::READABLE.0, 0, 0),
    );
    let mut iocb = m.read_iocb(IOCB);
    iocb.aio_flags = IocbFlags::IOCB_FLAG_RESFD.0;
    iocb.aio_resfd = efd.0 as u32;
    m.put_iocb(IOCB, &iocb);
    assert_eq!(m.submit(ctx, &[IOCB]), Ok(1));
    assert!(m.getevents(ctx, 0, 1, Some(Duration::ZERO)).is_empty());

    m.memory.put(BUF, b"!");
    block_on(m.kernel.write(&m.caller(), w, UserSlice::new(BUF, 1))).unwrap();
    let events = m.getevents(ctx, 1, 1, Some(Duration::from_secs(10)));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].obj, u64::from(IOCB));
    assert_eq!(
        events[0].res as u32 & PollMask::READABLE.0,
        PollMask::READABLE.0
    );
    let caller = m.caller();
    let count = m.kernel.read(&caller, efd, UserSliceMut::new(BUF + 8, 8));
    assert_eq!(block_on(count), Ok(8));
    assert_eq!(m.memory.get(BUF + 8, 8), 1u64.to_le_bytes());
}

#[test]
fn a_ready_file_completes_its_poll_in_io_submit() {
    let m = Machine::new();
    let ctx = m.ctx();
    let (_r, w) = pipe(&m.kernel, &m.caller(), OpenFlags(0));
    m.iocb(
        IOCB,
        IocbCmd::IOCB_CMD_POLL,
        w,
        (PollMask::WRITABLE.0, 0, 0),
    );
    assert_eq!(m.submit(ctx, &[IOCB]), Ok(1));
    assert_eq!(m.word(ctx + 12), 1);
    let events = m.getevents(ctx, 1, 1, Some(Duration::ZERO));
    assert_eq!(
        events[0].res as u32 & PollMask::WRITABLE.0,
        PollMask::WRITABLE.0
    );
}

#[test]
fn io_cancel_ends_a_waiting_poll_with_its_event_on_the_ring() {
    let m = Machine::new();
    let ctx = m.ctx();
    let (r, _w) = pipe(&m.kernel, &m.caller(), OpenFlags(0));
    m.iocb(
        IOCB,
        IocbCmd::IOCB_CMD_POLL,
        r,
        (PollMask::READABLE.0, 0, 0),
    );
    assert_eq!(m.submit(ctx, &[IOCB]), Ok(1));
    let cancel = |iocb: u32| {
        let result = GuestPtrMut::new(EVENTS + 0x100);
        block_on(
            m.kernel
                .io_cancel(&m.caller(), ctx, GuestPtrMut::new(iocb), result),
        )
    };
    assert_eq!(errno(cancel(IOCB)), Errno::EINPROGRESS);
    let events = m.getevents(ctx, 1, 1, Some(Duration::from_secs(10)));
    assert_eq!((events[0].obj, events[0].res), (u64::from(IOCB), 0));
    // Gone now, and nothing else is cancellable.
    assert_eq!(errno(cancel(IOCB)), Errno::EINVAL);
    let fd = m.file("c", b"");
    m.iocb(IOCB + 0x40, IocbCmd::IOCB_CMD_PREAD, fd, (BUF, 1, 0));
    assert_eq!(m.submit(ctx, &[IOCB + 0x40]), Ok(1));
    assert_eq!(errno(cancel(IOCB + 0x40)), Errno::EINVAL);
}

#[test]
fn getevents_waits_for_min_nr_or_the_timeout() {
    let m = Machine::new();
    let ctx = m.ctx();
    let fd = m.file("t", b"abc");
    m.iocb(IOCB, IocbCmd::IOCB_CMD_PREAD, fd, (BUF, 1, 0));
    assert_eq!(m.submit(ctx, &[IOCB]), Ok(1));
    let started = Instant::now();
    let events = m.getevents(ctx, 2, 4, Some(Duration::from_millis(50)));
    assert_eq!(events.len(), 1);
    assert!(started.elapsed() >= Duration::from_millis(40));
    let caller = m.caller();
    let bad = m.kernel.io_pgetevents_time64(
        &caller,
        ctx,
        2,
        1,
        GuestPtrMut::new(EVENTS),
        GuestPtrMut::NULL,
        GuestPtr::NULL,
    );
    assert_eq!(errno(block_on(bad)), Errno::EINVAL);
}

#[test]
fn a_full_ring_refuses_more_requests() {
    let m = Machine::new();
    m.setup(1).unwrap();
    let ctx = m.word(CTXP);
    let nr = m.word(ctx + 4);
    let fd = m.file("full", b"z");
    m.iocb(IOCB, IocbCmd::IOCB_CMD_PREAD, fd, (BUF, 1, 0));
    for _ in 0..nr - 1 {
        assert_eq!(m.submit(ctx, &[IOCB]), Ok(1));
    }
    assert_eq!(errno(m.submit(ctx, &[IOCB])), Errno::EAGAIN);
    // Reaping events makes room again, also when userspace moves the head itself.
    m.memory.put(ctx + 8, &m.word(ctx + 12).to_le_bytes());
    assert_eq!(m.submit(ctx, &[IOCB]), Ok(1));
}

#[test]
fn io_destroy_ends_the_context_and_gives_its_requests_back() {
    let m = Machine::new();
    let ctx = m.ctx();
    let (r, _w) = pipe(&m.kernel, &m.caller(), OpenFlags(0));
    m.iocb(
        IOCB,
        IocbCmd::IOCB_CMD_POLL,
        r,
        (PollMask::READABLE.0, 0, 0),
    );
    assert_eq!(m.submit(ctx, &[IOCB]), Ok(1));
    assert_eq!(block_on(m.kernel.io_destroy(&m.caller(), ctx)), Ok(0));
    assert_eq!(m.kernel.aio.nr(), 0);
    assert_eq!(errno(m.submit(ctx, &[IOCB])), Errno::EINVAL);
    assert_eq!(
        errno(block_on(m.kernel.io_destroy(&m.caller(), ctx))),
        Errno::EINVAL
    );
    // The ring is unmapped.
    assert_eq!(m.word(ctx + 16), 0);
}

#[test]
fn mremap_moves_a_whole_ring_and_its_context_id() {
    let m = Machine::new();
    let ctx = m.ctx();
    let len = 32 + m.word(ctx + 4) * 32;
    let caller = m.caller();
    let mremap = |addr: u32, old: u32, new: u32, flags: MremapFlags, to: u32| {
        block_on(m.kernel.mremap(&caller, addr, old, new, flags, to))
    };
    let may_move = MremapFlags::MREMAP_MAYMOVE;
    // A ring never grows, stays behind or moves in part.
    assert_eq!(errno(mremap(ctx, len, len * 2, may_move, 0)), Errno::EFAULT);
    let dont_unmap = may_move | MremapFlags::MREMAP_DONTUNMAP;
    assert_eq!(errno(mremap(ctx, len, len, dont_unmap, 0)), Errno::EINVAL);
    let fixed = may_move | MremapFlags::MREMAP_FIXED;
    let anon = MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS;
    let mmap = m
        .kernel
        .mmap(&caller, 0, len, ProtFlags(3), anon, Fd(-1), 0);
    let to = block_on(mmap).unwrap();
    // The whole ring moves, and the context goes by its new address.
    assert_eq!(mremap(ctx, len, len, fixed, to), Ok(to as i32));
    assert_eq!(m.word(to + 16), AioRingMisc::AIO_RING_MAGIC.0);
    let fd = m.file("moved", b"abc");
    m.iocb(IOCB, IocbCmd::IOCB_CMD_PREAD, fd, (BUF, 3, 0));
    assert_eq!(errno(m.submit(ctx, &[IOCB])), Errno::EINVAL);
    assert_eq!(m.submit(to, &[IOCB]), Ok(1));
    assert_eq!(m.word(to + 12), 1, "the event is on the moved ring");
    assert_eq!(block_on(m.kernel.io_destroy(&caller, to)), Ok(0));
}

/// fs/aio.c:aio_setup_ring sets VM_DONTCOPY; mm/mmap.c:dup_mmap skips that VMA.
#[test]
fn aio_rings_are_omitted_from_fork_and_the_parent_keeps_its_context() {
    let mut m = Machine::new();
    m.memory = MappedMemory::new(2, true);
    let ctx = m.ctx();
    let parent_mm = m.kernel.mm(m.task);
    assert!(
        parent_mm
            .regions()
            .iter()
            .any(|r| r.start == u64::from(ctx) && r.dont_copy)
    );
    let caller = m.caller();
    let child_pid = block_on(m.kernel.clone(
        &caller,
        wasixv2_abi::Signal::SIGCHLD.0 as u32,
        0,
        GuestPtrMut::NULL,
        0,
        GuestPtrMut::NULL,
    ))
    .unwrap();
    let child = match m.kernel.take_pending(m.task).unwrap() {
        GuestAction::Fork { child, .. } => child,
        action => panic!("expected fork, got {action:?}"),
    };
    assert!(child_pid > 0);
    let omitted = m.kernel.wipe_on_fork(child);
    assert!(omitted.iter().any(|r| r.contains(&u64::from(ctx))));
    assert!(m.kernel.shared_mappings(child).iter().all(|mapping| {
        !(u64::from(mapping.addr)..u64::from(mapping.addr) + mapping.len).contains(&u64::from(ctx))
    }));
    assert!(
        m.kernel
            .mm(child)
            .regions()
            .iter()
            .all(|r| !(r.start..r.end).contains(&u64::from(ctx)))
    );
    assert_eq!(
        m.kernel.page_access(child).unwrap().check(ctx, 1, false),
        Err(Fault)
    );
    assert_eq!(
        errno(block_on(
            m.kernel.io_destroy(&Caller::new(child, &m.memory), ctx)
        )),
        Errno::EINVAL
    );
    let fd = m.file("parent-aio", b"ok");
    m.iocb(IOCB, IocbCmd::IOCB_CMD_PREAD, fd, (BUF, 2, 0));
    assert_eq!(m.submit(ctx, &[IOCB]), Ok(1));
    assert_eq!(m.getevents(ctx, 1, 1, None)[0].res, 2);
    assert_eq!(m.memory.get(BUF, 2), b"ok");
    block_on(m.kernel.io_destroy(&caller, ctx)).unwrap();
}

/// The execution layer lets reads through its low guard for clang's null va_list;
/// Linux fs/aio.c:sys_io_cancel get_user(&iocb->aio_key) still faults there.
struct Guarded<'a>(&'a MappedMemory, u32);

impl UserMemory for Guarded<'_> {
    fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        self.0.read(addr, buf)
    }

    fn write<'a>(&'a self, addr: u32, data: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        match addr < self.1 {
            true => Box::pin(async { Err(Fault) }),
            false => self.0.write(addr, data),
        }
    }

    fn size(&self) -> LocalFuture<'_, u64> {
        self.0.size()
    }

    fn grow(&self, pages: u32) -> LocalFuture<'_, Option<u64>> {
        self.0.grow(pages)
    }

    fn low_guard(&self) -> LocalFuture<'_, u32> {
        let guard = self.1;
        Box::pin(async move { guard })
    }
}

#[test]
fn io_cancel_faults_on_guarded_and_wrapped_key_addresses() {
    let m = Machine::new();
    let ctx = m.ctx();
    let memory = Guarded(&m.memory, 0x100);
    let caller = Caller::new(m.task, &memory);
    for context in [ctx, 0] {
        for addr in [0, 16, u32::MAX - 4] {
            let ret = block_on(m.kernel.io_cancel(
                &caller,
                context,
                GuestPtrMut::new(addr),
                GuestPtrMut::NULL,
            ));
            assert_eq!(
                errno(ret),
                Errno::EFAULT,
                "context={context:#x} iocb={addr:#x}"
            );
        }
    }
    // A readable request with no submitted key is still EINVAL.
    let ret = block_on(
        m.kernel
            .io_cancel(&caller, ctx, GuestPtrMut::new(IOCB), GuestPtrMut::NULL),
    );
    assert_eq!(errno(ret), Errno::EINVAL);
}

/// fs/aio.c:aio_prep_rw passes RWF_NOWAIT through kiocb_set_rw_flags; pipe_read
/// returns EAGAIN rather than parking the submitting task (LTP io_submit04).
#[test]
fn nowait_aio_reads_complete_instead_of_waiting_for_a_pipe() {
    let m = Machine::new();
    let ctx = m.ctx();
    let (r, _w) = pipe(&m.kernel, &m.caller(), OpenFlags(0));
    m.memory.put(IOVECS, &BUF.to_le_bytes());
    m.memory.put(IOVECS + 4, &1u32.to_le_bytes());
    for (cmd, buf) in [
        (IocbCmd::IOCB_CMD_PREAD, BUF),
        (IocbCmd::IOCB_CMD_PREADV, IOVECS),
    ] {
        m.iocb(IOCB, cmd, r, (buf, 1, 0));
        let mut iocb = m.read_iocb(IOCB);
        iocb.aio_rw_flags = wasixv2_abi::RwfFlags::RWF_NOWAIT.0 as i32;
        m.put_iocb(IOCB, &iocb);
        m.memory.put(IOCBS, &IOCB.to_le_bytes());
        let caller = m.caller();
        let submitted = m.kernel.io_submit(&caller, ctx, 1, GuestPtr::new(IOCBS));
        assert_eq!(submitted.now_or_never(), Some(Ok(1)), "{cmd:?} blocked");
        let events = m.getevents(ctx, 1, 1, Some(Duration::ZERO));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].res, -i64::from(Errno::EAGAIN.0));
    }
}
