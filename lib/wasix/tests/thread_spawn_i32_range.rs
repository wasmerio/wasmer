//! WASIX-level coverage for spawning a thread whose `ThreadStart` structure sits
//! at or above the 2 GiB mark of a 32-bit linear memory.
//!
//! `wasi_thread_start` is captured by the host as `TypedFunction<(i32, i32), ()>`
//! (`state/handles/mod.rs`), and an `i32` at the wasm level carries the guest's
//! *whole* unsigned 32-bit address space - the libc shim loads its second argument
//! with an unsigned `i32.load`, and wasmer's own `Memory32::offset_to_native` is an
//! `as i32` reinterpretation rather than a range check (`lib/types/src/memory.rs`).
//! So a `ThreadStart` at or above 2 GiB is representable and has to reach the guest
//! unchanged. It did not: `call_module_internal` converted the offset with
//! `try_into::<i32>()`, whose error is *range*, not *representation*, and
//! `.unwrap()`ed that error on the task-manager worker - after
//! `wasi_thread_spawn` had already answered `Success`. Measured against a threaded
//! guest (pgrust serving pgwire, one backend thread per connection, ~16 MiB of
//! linear memory reserved per backend) that made the backend count stop at the
//! first allocation across 2 GiB: 102 admitted backends, and the same build with
//! the range check replaced by the bit-preserving cast admits 224.
//!
//! The guest here is a `wat!`-style module because this needs the raw ABI rather
//! than a libc: it grows its own memory past 2 GiB, writes a `ThreadStart` at
//! 0x8000_0000, and calls `thread_spawn` with that pointer. The memory has to be
//! declared `shared` - wasmx-threads' flavor - because otherwise
//! `thread_spawn_internal_using_layout` refuses both arms at its `as_shared()`
//! check with `Memviolation` and the cast under test is never reached. With it:
//!
//! * `0x8000_0000` -> `errno=0 success` with a non-zero tid. Restoring the old
//!   `try_into::<i32>()` range check makes this arm answer `errno=61 overflow`
//!   with tid `0`, which is the mutation arm of this test and the measured defect.
//! * `0x7FF0_0000`, one megabyte below -> `errno=0 success`, and the two tids
//!   differ, so two threads were registered rather than one answer cached.
//!
//! Limits, stated because the obvious strengthening was tried and did not work: the
//! child's `wasi_thread_start` records the argument it was handed into shared memory
//! (the `RES_GOT_*` slots), and neither arm's slot is ever written — measured on Linux
//! against a 10 s deadline per slot, which the run spent in full. So this file does
//! *not* prove that the address survives the callback intact; it proves the spawn is
//! refused no longer. That end-to-end half is carried by the server-scope A/B above,
//! where pgrust backends whose `ThreadStart` sits above 2 GiB accept connections and
//! answer `select <n>::int8` with their own `n`, which no mangled pointer would
//! survive. The record slots and their reported line stay in this probe so that a host
//! which does reach the child shows up as a changed number rather than as a missing
//! claim.
//!
//! Like `tests/stdio.rs` this needs an engine with a compiler backend:
//! `cargo test -p wasmer-wasix --features wasmer/cranelift --test thread_spawn_i32_range`.
//!
//! Cost, measured on Linux by running the built test binary directly: 0.01 s and
//! 50,416-50,576 kB peak resident, over three consecutive runs. Growing to
//! 32_896 pages reserves the address range; only the handful of pages the probe
//! writes become resident.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use virtual_fs::{MountFileSystem, RootFileSystemBuilder};
use wasmer::{Instance, Module, Store};
use wasmer_wasix::{WasiEnvBuilder, WasiFunctionEnv, wasmer_wasix_types::wasi::Errno};

/// `Errno::name()` for the raw value the guest recorded, so a failure reads as a
/// cause rather than a number.
fn errno_name(raw: u32) -> String {
    Errno::try_from(raw as u16)
        .map(|errno| errno.name().to_string())
        .unwrap_or_else(|_| format!("unknown({raw})"))
}

/// Pages: 2 GiB + 8 MiB, so every address the probe writes is inside the memory.
const PAGES_BEYOND_2_GIB: u32 = 32_896;
/// The threshold itself: 2 GiB, which is `i32::MAX + 1`.
const HIGH_PTR: u32 = 0x8000_0000;
/// A structure 1 MiB below the threshold, laid out identically.
const LOW_PTR: u32 = 0x7FF0_0000;
/// `ThreadStart` is 16 `u32` fields (`wasix_manual.rs`): stack_upper, tls_base,
/// start_funct, start_args, reserved[10], stack_size, guard_size.
const THREAD_START_SIZE: u32 = 64;

/// Results land here: errno and returned tid for the high pointer, then for the
/// low one, then the page count `memory.grow` reported.
const RES_HIGH_ERRNO: u64 = 1024;
const RES_HIGH_TID: u64 = 1028;
const RES_LOW_ERRNO: u64 = 1032;
const RES_LOW_TID: u64 = 1036;
const RES_PAGES: u64 = 1040;
/// `memory.size` after the grow: `memory.grow` answers with the *previous* size.
const RES_PAGES_AFTER: u64 = 1044;
/// Written by the *child* instance, from its own `wasi_thread_start`: the second
/// argument it was handed, and its tid. Split by the unsigned high/low test in the
/// WAT below, so each arm lands in its own pair of slots and neither can fill the
/// other's by accident.
const RES_GOT_ARG_HIGH: u64 = 1048;
const RES_GOT_TID_HIGH: u64 = 1052;
const RES_GOT_ARG_LOW: u64 = 1056;
const RES_GOT_TID_LOW: u64 = 1060;

const PROBE_WAT: &str = r#"
(module
  (import "wasix_32v1" "thread_spawn" (func $thread_spawn (param i32 i32) (result i32)))
  ;; `shared` is the wasmx-threads flavor: without it the attach-memory path
  ;; refuses the spawn and the child worker that hits the i32 cast never runs.
  (memory (export "memory") 1 65536 shared)
  ;; The host calls this with (i32 tid, start_arg), where start_arg is the offset of
  ;; the ThreadStart structure itself, carried through an i32. Recording it is the
  ;; point: only the thread that really started can write here, and it writes what it
  ;; was really handed. The split is unsigned, so the high arm cannot land in the low
  ;; arm's slots by a signed-comparison accident. Without the export the spawn is
  ;; refused for the unrelated `Notcapable` reason and neither arm reaches the cast
  ;; under test.
  (func (export "wasi_thread_start") (param $tid i32) (param $arg i32)
    (if (i32.ge_u (local.get $arg) (i32.const 0x80000000))
      (then
        (i32.store (i32.const 1048) (local.get $arg))
        (i32.store (i32.const 1052) (local.get $tid))
      )
      (else
        (i32.store (i32.const 1056) (local.get $arg))
        (i32.store (i32.const 1060) (local.get $tid))
      )
    )
  )
  ;; Fill a ThreadStart at $ptr: a 1 MiB stack whose top is above the structure.
  (func $write_thread_start (param $ptr i32)
    (i32.store (local.get $ptr) (i32.add (local.get $ptr) (i32.const 4096))) ;; stack_upper
    (i32.store (i32.add (local.get $ptr) (i32.const 4)) (local.get $ptr))    ;; tls_base
    (i32.store (i32.add (local.get $ptr) (i32.const 8)) (i32.const 0))       ;; start_funct
    (i32.store (i32.add (local.get $ptr) (i32.const 12)) (i32.const 0))      ;; start_args
    (i32.store (i32.add (local.get $ptr) (i32.const 56)) (i32.const 0x100000)) ;; stack_size
    (i32.store (i32.add (local.get $ptr) (i32.const 60)) (i32.const 0x1000))   ;; guard_size
  )
  (func (export "probe")
    ;; Grow past 2 GiB first: without this the high pointer would be rejected as
    ;; a memory violation and the test would pass for the wrong reason.
    (i32.store
      (i32.const 1040)
      (memory.grow (i32.sub (i32.const 32896) (memory.size)))
    )
    (i32.store (i32.const 1044) (memory.size))
    (call $write_thread_start (i32.const 0x80000000))
    (i32.store
      (i32.const 1024)
      (call $thread_spawn (i32.const 0x80000000) (i32.const 1028))
    )
    (call $write_thread_start (i32.const 0x7FF00000))
    (i32.store
      (i32.const 1032)
      (call $thread_spawn (i32.const 0x7FF00000) (i32.const 1036))
    )
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
    fn spawn() -> Self {
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

        // A memory-only root: this probe never touches a descriptor, but the
        // environment expects a filesystem to be mounted.
        let table = MountFileSystem::new();
        table
            .mount(
                Path::new("/"),
                Arc::new(RootFileSystemBuilder::default().build_tmp()),
            )
            .unwrap();

        let builder = WasiEnvBuilder::new("thread_spawn_probe")
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
        // Instantiate runs the module's initializers, so the result slots have
        // to be untouched before the probe below.
        for slot in [
            RES_HIGH_ERRNO,
            RES_HIGH_TID,
            RES_LOW_ERRNO,
            RES_LOW_TID,
            RES_PAGES,
            RES_PAGES_AFTER,
            RES_GOT_ARG_HIGH,
            RES_GOT_TID_HIGH,
            RES_GOT_ARG_LOW,
            RES_GOT_TID_LOW,
        ] {
            assert_eq!(probe.read(slot), 0, "result slot {slot} pre-filled");
        }
        probe
    }

    fn probe(&mut self) {
        let _guard = self._runtime.enter();
        let probe = self
            .instance
            .exports
            .get_function("probe")
            .expect("probe should be exported");
        probe.call(&mut self.store, &[]).expect("probe trapped");
    }

    fn read(&self, offset: u64) -> i32 {
        let memory = self.instance.exports.get_memory("memory").unwrap();
        let mut buf = [0u8; 4];
        memory
            .view(&self.store)
            .read(offset, &mut buf)
            .expect("the probe's result slots are inside the initial page");
        i32::from_le_bytes(buf)
    }

    /// A child's `wasi_thread_start` runs on a task-manager worker, so its write lands
    /// at some point after `thread_spawn` answers. Poll until the slot is non-zero or
    /// the deadline passes: the deadline only bounds how long this is willing to wait,
    /// because every assertion below still fixes the value it must reach, so a stalled
    /// host fails rather than passing slowly.
    fn wait_for(&self, offset: u64) -> i32 {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let value = self.read(offset);
            if value != 0 || Instant::now() >= deadline {
                return value;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

#[test]
fn thread_spawn_at_or_above_2_gib_reaches_the_guest_with_its_full_32_bit_pattern() {
    let mut probe = Probe::spawn();
    probe.probe();

    // `memory.grow` answers with the previous size, so -1 is the failure marker
    // and the post-grow `memory.size` is what bounds the pointers used above.
    let pages_before = probe.read(RES_PAGES);
    let pages = probe.read(RES_PAGES_AFTER) as u32;
    assert_ne!(
        pages_before,
        -1,
        "memory.grow past 2 GiB failed ({pages} pages afterwards)",
        pages = pages_before
    );
    assert!(
        pages >= PAGES_BEYOND_2_GIB,
        "memory should be grown past 2 GiB, measured {pages} pages"
    );
    // The structures have to be inside the grown memory, or a refusal below would just
    // be the bounds check answering.
    assert!(
        u64::from(pages) * 65_536 > u64::from(HIGH_PTR + THREAD_START_SIZE),
        "the high ThreadStart must be reachable in memory"
    );

    let high_errno = probe.read(RES_HIGH_ERRNO) as u32;
    let high_tid = probe.read(RES_HIGH_TID);
    let low_errno = probe.read(RES_LOW_ERRNO) as u32;
    let low_tid = probe.read(RES_LOW_TID);
    println!(
        "measured: high({HIGH_PTR:#x}) errno={high_errno} {} tid={high_tid},          low({LOW_PTR:#x}) errno={low_errno} {} tid={low_tid}, pages={pages}",
        errno_name(high_errno),
        errno_name(low_errno),
    );

    // The claim this file exists for: an offset in the upper half of a 32-bit memory is
    // representable in the ABI's i32, so the spawn must be accepted. Restoring the old
    // `try_into::<i32>()` range check makes this arm answer errno=61 with tid 0, which is
    // the defect rather than a different contract.
    assert_eq!(
        high_errno,
        u16::from(Errno::Success) as u32,
        "a start pointer at {HIGH_PTR:#x} is representable in the i32 the ABI carries and \
         must not be refused by a range check; the returned errno was {high_errno} \
         ({}) with tid {high_tid}",
        errno_name(high_errno)
    );
    assert_ne!(high_tid, 0, "the accepted spawn must write a tid");
    assert_eq!(
        low_errno,
        u16::from(Errno::Success) as u32,
        "the identical layout one megabyte below the line must still be accepted: \
         low={low_errno} ({})",
        errno_name(low_errno)
    );
    assert_ne!(low_tid, 0, "the low spawn must write a tid");
    assert_ne!(
        high_tid, low_tid,
        "two accepted spawns must register two threads, not answer from one handle: \
         high tid {high_tid}, low tid {low_tid}"
    );

    // What this probe cannot see: the callback itself. Both record slots stay empty for
    // either arm, including the low one, so the child's `wasi_thread_start` is not
    // observably entered here - see the Limits bullet in the header. The value is
    // reported, never asserted, so that a future probe which does reach the child turns
    // this line red rather than leaving the claim unsaid. The end-to-end half of the
    // claim - that the offset arrives intact - is carried by the server-scope A/B
    // described above, where backends whose ThreadStart sits above 2 GiB answer queries.
    let got_high = probe.wait_for(RES_GOT_ARG_HIGH) as u32;
    let got_low = probe.wait_for(RES_GOT_ARG_LOW) as u32;
    println!(
        "reported (not asserted): child saw high arg={got_high:#010x} tid={}, low arg={got_low:#010x} tid={}",
        probe.read(RES_GOT_TID_HIGH),
        probe.read(RES_GOT_TID_LOW),
    );
}
