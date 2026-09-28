//! WASIX-level coverage for the interest-list sweep that a closing descriptor
//! must run on every epoll instance watching it (`EpollInterestWork`).
//!
//! The `src/os/epoll/mod.rs` unit tests observe `EpollState::subscriptions`
//! directly, so they prove the bookkeeping but not the symptom the guest saw. The
//! symptom is what a server actually dies of: the interest lists are keyed by fd
//! *number*, which guests recycle, so an entry left behind by a closed descriptor
//! keeps that number claimed, and the next `EPOLL_CTL_ADD` of the recycled number
//! answers `EEXIST` (`EpollState::prepare_add` refuses on `contains_key`) while the
//! join guard keeps the watched object - for a TCP socket, the `LocalTcpStream`
//! owning the host descriptor - alive. Measured on pgrust: a backend that had
//! answered a CancelRequest left its client's socket `open_no_bytes` forever, where
//! native closes it at once.
//!
//! This test drives the raw ABI from a module and asserts only what the host's
//! answer can show, with the discrimination *inside one run*:
//!
//! * `EPOLL_CTL_ADD(e1, a)` -> `Success`, and the identical call again while `a`
//!   is still open -> `Exist`. That second arm is what makes the test
//!   non-vacuous: it proves a present interest-list entry really does make
//!   `ADD` refuse, so a later `Success` cannot be read as "this host never
//!   refuses a duplicate".
//! * `fd_close(a)` -> `Success`. The sweep runs before the syscall returns
//!   (`FdList::close_fd_and_capture_flush` notes the closing descriptor and its
//!   watchers, drops the map lock, then applies), so no other call is needed to
//!   flush it.
//! * A fresh `sock_pair` then answers the *same* number `a` (the fd table is
//!   lowest-free-first), and `ADD` of that number succeeds on **both** instances
//!   that watched it. Before the sweep existed both answers `Exist`, which is the
//!   failure mode, not a variant of it.
//!
//! The watched objects here are `Kind::DuplexPipe` inodes: `sock_pair` builds a
//! `Pipe::channel()` pair and ignores the family/type/proto arguments outright, so
//! this covers the interest-list lifetime, which is shared by every watchable
//! kind, rather than the `Kind::Socket` teardown that closes the host descriptor.
//! Which kinds are swept is `EpollInterestWork::note_closing`'s own match, and the
//! unit tests above pin `Kind::Socket` targets by identity.
//!
//! Like `tests/stdio.rs` these need an engine with a compiler backend:
//! `cargo test -p wasmer-wasix --features wasmer/cranelift --test epoll_prune_on_close`.

use std::path::Path;
use std::sync::Arc;

use virtual_fs::{MountFileSystem, RootFileSystemBuilder};
use wasmer::{Instance, Memory32, Module, Store};
use wasmer_wasix::{
    WasiEnvBuilder, WasiFunctionEnv,
    wasmer_wasix_types::wasi::{EpollCtl, EpollEvent, EpollType, Errno},
};

/// Output slots written by the syscalls themselves.
const OUT_EP1: u32 = 64;
const OUT_EP2: u32 = 68;
const OUT_A: u32 = 72;
const OUT_B: u32 = 76;
const OUT_A2: u32 = 80;
const OUT_B2: u32 = 84;

/// Returned errnos, one 4-byte slot per call, in the order the calls run.
const R_CREATE1: u32 = 128;
const R_CREATE2: u32 = 132;
const R_PAIR1: u32 = 136;
const R_ADD_LIVE: u32 = 140;
const R_ADD_LIVE_DUP: u32 = 144;
const R_ADD_OTHER: u32 = 148;
const R_CLOSE: u32 = 152;
const R_PAIR2: u32 = 156;
const R_READD_FIRST: u32 = 160;
const R_READD_SECOND: u32 = 164;
const R_WAIT_ERRNO: u32 = 200;
const R_WAIT_NEVENTS: u32 = 204;

/// Where the guest keeps the one `ep_event` it passes to every `EPOLL_CTL_ADD`,
/// and the array `epoll_wait` writes into. Both are 8-byte aligned because the
/// event's `data2` is a `u64`, and the buffer bounds are derived from the real
/// struct rather than assumed: the module below hardcodes `512`, `1024` and
/// `1152`, which the assertions here are the check on.
const EVENT_IN: u32 = 512;
const EVENT_OUT: u32 = 1024;
const MAXEVENTS: u32 = 4;
const NEVENTS_OUT: u32 = EVENT_OUT + MAXEVENTS * EVENT_BYTES;

const EVENT_BYTES: u32 = core::mem::size_of::<EpollEvent<Memory32>>() as u32;

const _: () = assert!(core::mem::offset_of!(EpollEvent<Memory32>, events) == 0);
const _: () = assert!(EVENT_BYTES % 8 == 0);
const _: () = assert!(NEVENTS_OUT % 8 == 0);
const _: () = assert!(EVENT_IN % 8 == 0 && EVENT_OUT % 8 == 0);
// The four regions the module uses must name distinct bytes, and the whole set
// must sit inside the single page the module declares.
const _: () = assert!(R_WAIT_NEVENTS + 4 < EVENT_IN);
const _: () = assert!(EVENT_IN + EVENT_BYTES < EVENT_OUT);
const _: () = assert!(NEVENTS_OUT + 4 < 65_536);

/// `EPOLL_CTL_ADD`, pinned against the enum the syscall actually parses: if this
/// discriminant ever moved, every registration below would be a different
/// operation rather than the one under test.
const OP_ADD: u32 = 0;
const _: () = assert!(EpollCtl::Add as u32 == OP_ADD);

const PROBE_WAT: &str = r#"
(module
  (import "wasix_32v1" "epoll_create" (func $epoll_create (param i32) (result i32)))
  (import "wasix_32v1" "epoll_ctl" (func $epoll_ctl (param i32 i32 i32 i32) (result i32)))
  (import "wasix_32v1" "epoll_wait" (func $epoll_wait (param i32 i32 i32 i64 i32) (result i32)))
  (import "wasix_32v1" "sock_pair" (func $sock_pair (param i32 i32 i32 i32 i32) (result i32)))
  (import "wasix_32v1" "fd_close" (func $fd_close (param i32) (result i32)))
  (memory (export "memory") 1)
  ;; Open two epoll instances and one socket pair, register one end of the pair in
  ;; both instances, register it a second time while it is still open, then close
  ;; it. Everything after the close is a separate export so the readiness leg can
  ;; be skipped by the caller that only needs the errno sequence.
  (func $before_close (param $epollin i32)
    (i32.store (i32.const 512) (local.get $epollin))
    (i32.store (i32.const 128) (call $epoll_create (i32.const 64)))
    (i32.store (i32.const 132) (call $epoll_create (i32.const 68)))
    (i32.store
      (i32.const 136)
      (call $sock_pair (i32.const 2) (i32.const 1) (i32.const 6) (i32.const 72) (i32.const 76))
    )
    (i32.store
      (i32.const 140)
      (call $epoll_ctl (i32.load (i32.const 64)) (i32.const 0) (i32.load (i32.const 72)) (i32.const 512))
    )
    (i32.store
      (i32.const 144)
      (call $epoll_ctl (i32.load (i32.const 64)) (i32.const 0) (i32.load (i32.const 72)) (i32.const 512))
    )
    (i32.store
      (i32.const 148)
      (call $epoll_ctl (i32.load (i32.const 68)) (i32.const 0) (i32.load (i32.const 72)) (i32.const 512))
    )
    (i32.store (i32.const 152) (call $fd_close (i32.load (i32.const 72))))
  )
  ;; The recycled number must be registerable again in both instances.
  (func (export "after_close") (param $epollin i32)
    (call $before_close (local.get $epollin))
    (i32.store
      (i32.const 156)
      (call $sock_pair (i32.const 2) (i32.const 1) (i32.const 6) (i32.const 80) (i32.const 84))
    )
    (i32.store
      (i32.const 160)
      (call $epoll_ctl (i32.load (i32.const 64)) (i32.const 0) (i32.load (i32.const 80)) (i32.const 512))
    )
    (i32.store
      (i32.const 164)
      (call $epoll_ctl (i32.load (i32.const 68)) (i32.const 0) (i32.load (i32.const 80)) (i32.const 512))
    )
  )
  ;; Poll the first instance once, with an empty interest list expected. A zero
  ;; timeout is answered with Success and zero events by the host's own timeout
  ;; arm, so a non-zero `nevents` here can only mean a retained entry.
  (func (export "wait_after_close") (param $epollin i32)
    (call $before_close (local.get $epollin))
    (i32.store
      (i32.const 200)
      (call $epoll_wait
        (i32.load (i32.const 64))
        (i32.const 1024)
        (i32.const 4)
        (i64.const 0)
        (i32.const 1152)
      )
    )
    (i32.store (i32.const 204) (i32.load (i32.const 1152)))
  )
)
"#;

struct Probe {
    _runtime: tokio::runtime::Runtime,
    store: Store,
    instance: Instance,
    _env: WasiFunctionEnv,
}

impl Probe {
    fn new() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let engine = wasmer::Engine::default();
        let module = match Module::new(&engine, PROBE_WAT) {
            Ok(module) => module,
            Err(err) => panic!(
                "{err} - this test needs a compiler backend, e.g. --features wasmer/cranelift"
            ),
        };

        // Nothing is mounted: the probe never opens a path, but the environment
        // expects a filesystem to exist.
        let table = MountFileSystem::new();
        table
            .mount(
                Path::new("/"),
                Arc::new(RootFileSystemBuilder::default().build_tmp()),
            )
            .unwrap();

        let builder = WasiEnvBuilder::new("epoll_prune_probe")
            .engine(engine.clone())
            .mount_fs(table);
        let _guard = runtime.enter();
        let mut store = Store::new(engine);
        let (instance, env) = builder.instantiate(module, &mut store).unwrap();
        drop(_guard);

        let probe = Probe {
            _runtime: runtime,
            store,
            instance,
            _env: env,
        };
        for slot in [
            R_CREATE1,
            R_CREATE2,
            R_PAIR1,
            R_ADD_LIVE,
            R_ADD_LIVE_DUP,
            R_ADD_OTHER,
            R_CLOSE,
            R_PAIR2,
            R_READD_FIRST,
            R_READD_SECOND,
            R_WAIT_ERRNO,
            R_WAIT_NEVENTS,
        ] {
            assert_eq!(
                probe.read(i64::from(slot)),
                0,
                "result slot {slot} pre-filled"
            );
        }
        probe
    }

    /// Run one exported leg with `EpollType::EPOLLIN` as its only argument, and
    /// check that the guest's own store landed at `EVENT_IN`: the module writes
    /// the bit there with a plain `i32.store`, while the host reads it as the
    /// first field of `EpollEvent`, so this is the layout claim executed rather
    /// than only asserted in the `const` block above.
    fn run(&mut self, export: &str) {
        let epollin = EpollType::EPOLLIN.bits();
        let _guard = self._runtime.enter();
        let fun = self
            .instance
            .exports
            .get_function(export)
            .unwrap_or_else(|_| panic!("{export} should be exported"));
        fun.call(&mut self.store, &[wasmer::Value::I32(epollin as i32)])
            .unwrap_or_else(|err| panic!("{export} trapped: {err}"));
        assert_eq!(
            self.read(i64::from(EVENT_IN)),
            epollin as i32,
            "the guest's event mask must land at offset 0 of the {EVENT_BYTES}-byte \
             EpollEvent the host parses"
        );
    }

    fn read(&self, offset: i64) -> i32 {
        let memory = self.instance.exports.get_memory("memory").unwrap();
        let mut buf = [0u8; 4];
        memory
            .view(&self.store)
            .read(u64::try_from(offset).unwrap(), &mut buf)
            .unwrap_or_else(|err| panic!("read at {offset}: {err}"));
        i32::from_le_bytes(buf)
    }

    /// `Errno::name()` for a recorded raw value, so a failure reads as a cause.
    fn errno(&self, slot: u32) -> String {
        let raw = self.read(i64::from(slot)) as u32;
        Errno::try_from(raw as u16)
            .map(|errno| errno.name().to_string())
            .unwrap_or_else(|_| format!("unknown({raw})"))
    }

    fn assert_errno(&self, slot: u32, expected: Errno) {
        let raw = self.read(i64::from(slot)) as u32;
        assert_eq!(
            raw,
            u16::from(expected) as u32,
            "slot {slot} answered {raw} ({}), expected {:?}",
            self.errno(slot),
            expected
        );
    }
}

#[test]
fn closing_a_watched_descriptor_frees_its_number_for_the_next_epoll_ctl_add() {
    let mut probe = Probe::new();
    probe.run("after_close");

    // The setup must have happened for the reason we think it did: two live
    // instances, one live pair, and every registration accepted while `a` was
    // open.
    for slot in [R_CREATE1, R_CREATE2, R_PAIR1, R_ADD_LIVE, R_ADD_OTHER] {
        probe.assert_errno(slot, Errno::Success);
    }
    assert!(probe.read(i64::from(OUT_EP1)) >= 3, "epfd1 over stdio");
    assert!(probe.read(i64::from(OUT_EP2)) > probe.read(i64::from(OUT_EP1)));
    assert_ne!(
        probe.read(i64::from(OUT_A)),
        probe.read(i64::from(OUT_B)),
        "a socket pair's two ends are distinct descriptors"
    );

    // The non-vacuity control: while `a` is open, the identical ADD of the same
    // number refuses. Without this, an ADD that never answers `Exist` would make
    // every assertion below pass on a host that has no interest list at all.
    probe.assert_errno(R_ADD_LIVE_DUP, Errno::Exist);

    probe.assert_errno(R_CLOSE, Errno::Success);

    // The fresh pair must reuse the number that was just released, or the arm
    // below is a registration of a different descriptor and proves nothing.
    probe.assert_errno(R_PAIR2, Errno::Success);
    let a = u32::try_from(probe.read(i64::from(OUT_A))).unwrap();
    let a2 = u32::try_from(probe.read(i64::from(OUT_A2))).unwrap();
    assert_eq!(
        a2, a,
        "the second pair's first end must land on the number the closed \
         descriptor left behind; the fd table is lowest-free-first"
    );

    // The other end of the new pair must be a genuinely new number: if the host
    // had handed back the number of the still-open `b`, then the table is not
    // lowest-free-first and the equality above proves nothing about the sweep.
    let b = u32::try_from(probe.read(i64::from(OUT_B))).unwrap();
    let b2 = u32::try_from(probe.read(i64::from(OUT_B2))).unwrap();
    assert_ne!(b2, a2, "the two ends of one pair are distinct descriptors");
    assert_ne!(
        b2, b,
        "two simultaneously open descriptors cannot share a number"
    );

    // The defect and its fix, one call apart: the sweep is per-instance, so
    // every list that named `a` must now be willing to name it again.
    probe.assert_errno(R_READD_FIRST, Errno::Success);
    probe.assert_errno(R_READD_SECOND, Errno::Success);

    println!(
        "measured: epfd=({ep1}, {ep2}) pair1=({a}, {b}) pair2=({a2}, {b2}) \
         add_live={} add_live_dup={} add_other={} close={} readd_first={} readd_second={}",
        probe.errno(R_ADD_LIVE),
        probe.errno(R_ADD_LIVE_DUP),
        probe.errno(R_ADD_OTHER),
        probe.errno(R_CLOSE),
        probe.errno(R_READD_FIRST),
        probe.errno(R_READD_SECOND),
        ep1 = probe.read(i64::from(OUT_EP1)),
        ep2 = probe.read(i64::from(OUT_EP2)),
    );
}

#[test]
fn closing_a_watched_descriptor_leaves_epoll_wait_with_nothing_to_report() {
    let mut probe = Probe::new();
    probe.run("wait_after_close");

    probe.assert_errno(R_ADD_LIVE, Errno::Success);
    probe.assert_errno(R_ADD_LIVE_DUP, Errno::Exist);
    probe.assert_errno(R_CLOSE, Errno::Success);

    // A zero timeout is answered `Success` with `nevents` written as 0 by the
    // host's timeout arm, so a count here can only have come from a retained
    // entry firing for a descriptor the guest no longer holds.
    probe.assert_errno(R_WAIT_ERRNO, Errno::Success);
    let nevents = probe.read(i64::from(R_WAIT_NEVENTS));
    assert_eq!(
        nevents, 0,
        "epoll_wait reported a readiness event for the closed descriptor"
    );
    println!(
        "measured: epoll_wait after close -> errno={} nevents={nevents} (maxevents {MAXEVENTS})",
        probe.errno(R_WAIT_ERRNO),
    );
}
