// With `v8-default` (the Windows test lane) `Store::default()` is a V8 store,
// which these native scenarios cannot run on.
#![cfg(all(
    feature = "experimental-async",
    feature = "sys",
    not(feature = "v8-default"),
    not(target_arch = "wasm32")
))]
//! Thread state of native `call_async` calls that overlap on one thread.
//!
//! Several `call_async` futures can be suspended on one thread and resumed in
//! any order. What the runtime keeps per running call (the trap handler the
//! signal handler consults, the interrupt registry's active stores, the
//! current coroutine) has to follow the coroutine that is running, not the
//! order in which the calls started.
//!
//! A regression here ends with the process killed by a signal, so those
//! scenarios run in a child process (this test binary, re-executed) and are
//! reported as failed tests.

use std::{
    cell::{Cell, RefCell},
    future::Future,
    io::Read,
    pin::Pin,
    process::{Command, Stdio},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};

use wasmer::{
    AsStoreAsync, Function, FunctionType, Instance, Module, RuntimeError, Store, StoreAsync,
    StoreAsyncWriteLock, Type, TypedFunction, Value, imports,
};
use wasmer_vm::TrapCode;

const WAT: &str = r#"
(module
  (import "h" "wait" (func $wait (param i32)))
  (import "h" "host" (func $host))
  (import "h" "keep_spinning" (func $keep_spinning (result i32)))
  (memory 1)
  (global $after (export "after") (mut i32) (i32.const 0))
  (func $rec (param i32) (result i32)
    (i32.add (call $rec (i32.add (local.get 0) (i32.const 1))) (i32.const 1)))
  (func $trap (param $how i32)
    (if (i32.eq (local.get $how) (i32.const 1))
      (then (drop (i32.load (i32.const 0x7ffffff0)))))
    (if (i32.eq (local.get $how) (i32.const 2))
      (then (drop (i32.div_u (i32.const 1) (i32.const 0)))))
    (if (i32.eq (local.get $how) (i32.const 3))
      (then (drop (call $rec (i32.const 0)))))
    (if (i32.eq (local.get $how) (i32.const 4))
      (then unreachable)))
  ;; Waits for a gate, then traps as asked or returns 42.
  (func (export "run") (param $gate i32) (param $how i32) (result i32)
    (call $wait (local.get $gate))
    (global.set $after (i32.add (global.get $after) (i32.const 1)))
    (call $trap (local.get $how))
    (i32.const 42))
  ;; Calls back into the host, then traps as asked or returns 7.
  (func (export "via_host") (param $how i32) (result i32)
    (call $host)
    (call $trap (local.get $how))
    (i32.const 7))
  ;; Waits for a gate, then runs guest code only, `stretch` iterations at a
  ;; time: once, or with `ask` for as long as the host says.
  (func (export "spin") (param $gate i32) (param $stretch i32) (param $ask i32) (result i32)
    (local $n i32)
    (call $wait (local.get $gate))
    (drop (call $keep_spinning))
    (loop $again
      (local.set $n (local.get $stretch))
      (loop $l
        (local.set $n (i32.sub (local.get $n) (i32.const 1)))
        (br_if $l (local.get $n)))
      (if (local.get $ask)
        (then (br_if $again (call $keep_spinning)))))
    (i32.const 42)))
"#;

/// Pending until its flag is set. It never wakes: the tests poll by hand.
struct Gate(Rc<Cell<bool>>);
impl Future for Gate {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
        if self.0.get() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

struct SetOnDrop(Rc<Cell<bool>>);
impl Drop for SetOnDrop {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

/// A waker that counts how often it is used.
#[derive(Default)]
struct Wakes(AtomicUsize);
impl Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
impl Wakes {
    fn count(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
struct Spin {
    /// Set once the guest is in `spin`.
    started: AtomicBool,
    /// Tells a `spin` that asks to return.
    stop: AtomicBool,
}

type Gates = [Rc<Cell<bool>>; 2];
type CallFuture = Pin<Box<dyn Future<Output = Result<Box<[Value]>, RuntimeError>>>>;
type HostHook = Box<dyn FnMut() -> Result<(), RuntimeError>>;

fn gates() -> Gates {
    [Rc::new(Cell::new(false)), Rc::new(Cell::new(false))]
}

struct Guest {
    store: StoreAsync,
    instance: Instance,
    /// Set when the host future of `wait(i)` is dropped.
    host_future_dropped: Gates,
    /// What the `host` import does. It runs on the stack of the call.
    host: Rc<RefCell<Option<HostHook>>>,
    #[cfg(all(unix, feature = "experimental-host-interrupt"))]
    spin: Arc<Spin>,
    #[cfg(all(unix, feature = "experimental-host-interrupt"))]
    interrupter: wasmer::Interrupter,
}

impl Guest {
    fn new(gates: &Gates) -> Self {
        let mut store = Store::default();
        let module = Module::new(&store, WAT).unwrap();
        let host_future_dropped = self::gates();
        let wait = {
            let gates = gates.clone();
            let dropped = host_future_dropped.clone();
            Function::new_async(
                &mut store,
                FunctionType::new(vec![Type::I32], vec![]),
                move |vals| {
                    let idx = vals[0].unwrap_i32() as usize;
                    let gate = gates[idx].clone();
                    let dropped = SetOnDrop(dropped[idx].clone());
                    async move {
                        let _dropped = dropped;
                        Gate(gate).await;
                        Ok(vec![])
                    }
                },
            )
        };
        let host: Rc<RefCell<Option<HostHook>>> = Default::default();
        let host_import = {
            let host = host.clone();
            // The hook runs in the body, on the stack of the call; the
            // returned future is ready.
            Function::new_async(&mut store, FunctionType::new(vec![], vec![]), move |_| {
                let result = host.borrow_mut().as_mut().map_or(Ok(()), |hook| hook());
                async move { result.map(|()| vec![]) }
            })
        };
        let spin = Arc::new(Spin::default());
        let keep_spinning = {
            let spin = spin.clone();
            Function::new_typed(&mut store, move || -> i32 {
                spin.started.store(true, Ordering::SeqCst);
                !spin.stop.load(Ordering::SeqCst) as i32
            })
        };
        let instance = Instance::new(
            &mut store,
            &module,
            &imports! { "h" => {
                "wait" => wait,
                "host" => host_import,
                "keep_spinning" => keep_spinning,
            } },
        )
        .unwrap();
        Self {
            #[cfg(all(unix, feature = "experimental-host-interrupt"))]
            interrupter: store.interrupter(),
            #[cfg(all(unix, feature = "experimental-host-interrupt"))]
            spin,
            store: store.into_async(),
            instance,
            host_future_dropped,
            host,
        }
    }

    fn on_host(&self, hook: impl FnMut() -> Result<(), RuntimeError> + 'static) {
        *self.host.borrow_mut() = Some(Box::new(hook));
    }

    fn run(&self, gate: i32, how: i32) -> CallFuture {
        self.call("run", &[gate, how])
    }

    fn call(&self, name: &str, args: &[i32]) -> CallFuture {
        let f = self.instance.exports.get_function(name).unwrap().clone();
        let args = args.iter().map(|v| Value::I32(*v)).collect();
        Box::pin(f.call_async(&self.store, args))
    }

    /// For synchronous calls.
    fn into_store(self) -> (Store, Instance) {
        let store = self.store.into_store().expect("store is free");
        (store, self.instance)
    }

    fn try_write(&self) -> Option<StoreAsyncWriteLock> {
        match Box::pin(self.store.write_lock())
            .as_mut()
            .poll(&mut Context::from_waker(&futures::task::noop_waker()))
        {
            Poll::Ready(guard) => Some(guard),
            Poll::Pending => None,
        }
    }

    fn after(&self) -> i32 {
        let mut guard = self.try_write().expect("store is not locked");
        self.instance
            .exports
            .get_global("after")
            .unwrap()
            .get(&mut guard)
            .unwrap_i32()
    }
}

fn poll_once<T>(f: &mut Pin<Box<dyn Future<Output = T>>>) -> Poll<T> {
    f.as_mut()
        .poll(&mut Context::from_waker(&futures::task::noop_waker()))
}

fn expect_pending(what: &str, f: &mut CallFuture) {
    assert!(poll_once(f).is_pending(), "{what} should be suspended");
}

fn expect_value(what: &str, f: &mut CallFuture, value: i32) {
    match poll_once(f) {
        Poll::Ready(Ok(v)) => assert_eq!(v[0].unwrap_i32(), value, "{what}"),
        other => panic!("{what}: expected {value}, got {other:?}"),
    }
}

fn expect_trap(what: &str, f: &mut CallFuture, code: TrapCode) {
    match poll_once(f) {
        Poll::Ready(Err(e)) => assert_trap(what, &e, code),
        other => panic!("{what}: expected trap {code:?}, got {other:?}"),
    }
}

fn expect_host_panic(f: &mut CallFuture) {
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| poll_once(f)));
    assert!(caught.is_err(), "the host panic reaches the poller");
}

fn assert_trap(what: &str, e: &RuntimeError, code: TrapCode) {
    let message = e.message();
    assert_eq!(e.clone().to_trap(), Some(code), "{what}: {message}");
}

/// Guest traps and the argument of `$trap` that raises them. Whether a trap
/// is raised through a signal depends on the target and the compiler; the
/// ones that are not pass with or without per-activation state.
const TRAPS: [(&str, i32, TrapCode); 4] = [
    ("oob", 1, TrapCode::HeapAccessOutOfBounds),
    ("div0", 2, TrapCode::IntegerDivisionByZero),
    ("overflow", 3, TrapCode::StackOverflow),
    ("unreachable", 4, TrapCode::UnreachableCodeReached),
];

fn trap(name: &str) -> (i32, TrapCode) {
    let (_, how, code) = *TRAPS.iter().find(|t| t.0 == name).unwrap();
    (how, code)
}

/// Calls A and B are started in that order and both suspend. One of them
/// traps after the other one ran, returned or was dropped in between.
fn interleaved_trap(order: &str, stores: &str, trap_name: &str) {
    let (how, code) = trap(trap_name);
    let g = gates();
    let guest_a = Guest::new(&g);
    let other = (stores == "two_stores").then(|| Guest::new(&g));
    // With one store both calls share it; its lock is released whenever the
    // running call suspends, which is what lets the other one run.
    let guest_b = other.as_ref().unwrap_or(&guest_a);

    let (how_a, how_b) = match order {
        "a_traps_while_b_suspended" => (how, 0),
        _ => (0, how),
    };
    let mut a = guest_a.run(0, how_a);
    let mut b = guest_b.run(1, how_b);
    expect_pending("A", &mut a);
    expect_pending("B", &mut b);
    match order {
        "a_traps_while_b_suspended" => {
            g[0].set(true);
            expect_trap("A", &mut a, code);
            g[1].set(true);
            expect_value("B", &mut b, 42);
        }
        "b_traps_after_a_returned" => {
            g[0].set(true);
            expect_value("A", &mut a, 42);
            g[1].set(true);
            expect_trap("B", &mut b, code);
        }
        "b_traps_after_a_dropped" => {
            drop(a);
            g[1].set(true);
            expect_trap("B", &mut b, code);
        }
        other => panic!("unknown order {other}"),
    }

    // The stores are still usable, and a later trap is still caught.
    g[0].set(true);
    for guest in [Some(&guest_a), other.as_ref()].into_iter().flatten() {
        expect_value("later call", &mut guest.run(0, 0), 42);
        expect_trap("later trap", &mut guest.run(0, how), code);
    }
}

/// The calls are polled from inside a host function, so the poller has Wasm
/// frames and a trap handler of its own: those of a synchronous call, or of
/// another `call_async`. One call traps, one is dropped while suspended, one
/// is unwound by a host panic and one is left suspended. Then the poller
/// returns to its guest, which traps.
fn poller_in_wasm(poller: &str, trap_name: &str) {
    let (how, code) = trap(trap_name);
    let g = gates();
    let guest_a = Guest::new(&g);
    let guest_b = Guest::new(&g);
    let left_suspended: Rc<RefCell<Option<CallFuture>>> = Default::default();
    let outer = Guest::new(&g);
    outer.on_host({
        let (g, left_suspended) = (g.clone(), left_suspended.clone());
        move || {
            let mut a = guest_a.run(0, how);
            let mut b = guest_b.run(1, 0);
            expect_pending("A", &mut a);
            expect_pending("B", &mut b);
            g[0].set(true);
            expect_trap("A", &mut a, code);
            let mut dropped = guest_a.run(1, 0);
            expect_pending("dropped", &mut dropped);
            drop(dropped);
            guest_a.on_host(|| panic!("host function panicked"));
            expect_host_panic(&mut guest_a.call("via_host", &[0]));
            *left_suspended.borrow_mut() = Some(b);
            Ok(())
        }
    });

    if poller == "sync" {
        let (mut store, instance) = outer.into_store();
        let via_host = instance.exports.get_function("via_host").unwrap();
        let err = via_host.call(&mut store, &[Value::I32(how)]).unwrap_err();
        assert_trap("outer", &err, code);
    } else {
        expect_trap("outer", &mut outer.call("via_host", &[how]), code);
    }

    let mut b = left_suspended.borrow_mut().take().unwrap();
    g[1].set(true);
    expect_value("B", &mut b, 42);
}

/// A host panic unwinds a `call_async`; the embedder catches it. A later
/// synchronous call that reaches a pending async import must report that it
/// cannot suspend instead of using the dead coroutine.
fn host_panic_then_sync_call() {
    let g = gates();
    let guest = Guest::new(&g);
    guest.on_host(|| panic!("host function panicked"));
    expect_host_panic(&mut guest.call("via_host", &[0]));

    let (mut store, instance) = guest.into_store();
    let run = instance.exports.get_function("run").unwrap();
    let err = run
        .call(&mut store, &[Value::I32(0), Value::I32(0)])
        .unwrap_err();
    assert_trap("sync call", &err, TrapCode::YieldOutsideAsyncContext);
}

/// A call on one store, running on a coroutine, synchronously calls into
/// another store whose async import is pending. A store entered synchronously
/// cannot be suspended, not even by the coroutine beneath it: the import
/// traps, as it does without a coroutine, and both stores stay usable. An
/// import that is ready completes through the nested entry.
fn pending_import_under_nested_sync_entry() {
    let g = gates();
    let (mut inner_store, inner) = Guest::new(&g).into_store();
    let outer = Guest::new(&g);
    outer.on_host(move || {
        let run = inner.exports.get_function("run").unwrap();
        run.call(&mut inner_store, &[Value::I32(0), Value::I32(0)])
            .map(|_| ())
    });
    let mut call = outer.call("via_host", &[0]);
    expect_trap("outer", &mut call, TrapCode::YieldOutsideAsyncContext);
    drop(call);
    g[0].set(true);
    expect_value("ready", &mut outer.call("via_host", &[0]), 7);
    outer.into_store();
}

/// A runs guest code while B, which entered Wasm after it, is suspended, and
/// one of the two stores is interrupted from another thread. The signal has
/// to be judged against the store that is running: aimed at B it must leave A
/// alone, aimed at A it must trap it.
#[cfg(all(unix, feature = "experimental-host-interrupt"))]
fn interrupt_while_another_call_is_suspended(target: &str) {
    use wasmer_vm::interrupt_registry::{self, InterruptError};

    let at_running = target == "running";
    let g = gates();
    let guest_a = Guest::new(&g);
    let guest_b = Guest::new(&g);
    // Aimed at B, A spins until the signal was taken. Aimed at A, it spins
    // for much longer than that takes and then returns without going back to
    // the host, where a missed interrupt would be noticed after all.
    let spin_args = if at_running {
        [0, i32::MAX, 0]
    } else {
        [0, 1 << 20, 1]
    };
    let mut a = guest_a.call("spin", &spin_args);
    let mut b = guest_b.run(1, 0);
    expect_pending("A", &mut a);
    expect_pending("B", &mut b);

    let spin = guest_a.spin.clone();
    let interrupter = if at_running { &guest_a } else { &guest_b }
        .interrupter
        .clone();
    let store_b = guest_b.store.store_id();
    let other = std::thread::spawn(move || {
        while !spin.started.load(Ordering::SeqCst) {
            std::thread::yield_now();
        }
        interrupter.interrupt();
        if !at_running {
            // A second request is accepted only once the thread has taken
            // the signal of the first. Its own signal is harmless wherever
            // it lands: the thread is running A or host code, never B.
            loop {
                match interrupt_registry::interrupt(store_b) {
                    Ok(()) => break,
                    Err(InterruptError::OtherInterruptInProgress) => std::thread::yield_now(),
                    Err(e) => panic!("{e}"),
                }
            }
            spin.stop.store(true, Ordering::SeqCst);
        }
    });

    g[0].set(true);
    if at_running {
        expect_trap("A", &mut a, TrapCode::HostInterrupt);
        g[1].set(true);
        expect_value("B", &mut b, 42);
    } else {
        expect_value("A", &mut a, 42);
        expect_trap("B", &mut b, TrapCode::HostInterrupt);
    }
    other.join().unwrap();
}

const SCENARIO_ENV: &str = "WASMER_ACTIVATION_SCENARIO";
const SCENARIO_COMPLETED: &str = "scenario completed:";
const SCENARIO_TIMEOUT: Duration = Duration::from_secs(60);

/// Runs the scenario named by the environment; does nothing otherwise.
#[test]
fn child_scenario() {
    let Ok(scenario) = std::env::var(SCENARIO_ENV) else {
        return;
    };
    let parts: Vec<&str> = scenario.split(':').collect();
    match parts[0] {
        "trap" => interleaved_trap(parts[1], parts[2], parts[3]),
        "poller" => poller_in_wasm(parts[1], parts[2]),
        "host_panic_then_sync_call" => host_panic_then_sync_call(),
        "nested_sync_entry" => pending_import_under_nested_sync_entry(),
        #[cfg(all(unix, feature = "experimental-host-interrupt"))]
        "interrupt" => interrupt_while_another_call_is_suspended(parts[1]),
        other => panic!("unknown scenario {other}"),
    }
    println!("{SCENARIO_COMPLETED} {scenario}");
}

/// Runs one scenario in a child process and describes how it failed.
fn run_child(scenario: &str) -> Result<(), String> {
    fn drain(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            pipe.read_to_end(&mut bytes).ok();
            String::from_utf8_lossy(&bytes).into_owned()
        })
    }

    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "child_scenario",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(SCENARIO_ENV, scenario)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = drain(child.stdout.take().unwrap());
    let stderr = drain(child.stderr.take().unwrap());
    let deadline = Instant::now() + SCENARIO_TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= deadline {
            child.kill().ok();
            child.wait().ok();
            break None;
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let (stdout, stderr) = (stdout.join().unwrap(), stderr.join().unwrap());

    let how = match status {
        // Exit status 0 alone is not enough: the harness also exits with it
        // when no test matched.
        Some(status) if status.success() => {
            if stdout.contains(&format!("{SCENARIO_COMPLETED} {scenario}")) {
                return Ok(());
            }
            "did not run".to_string()
        }
        Some(status) => status.to_string(),
        None => format!("timed out after {SCENARIO_TIMEOUT:?}"),
    };
    Err(format!("{how}\n{}", stderr.trim_end()))
}

fn run_children(scenarios: impl IntoIterator<Item = String>) {
    let mut failures = 0;
    for scenario in scenarios {
        match run_child(&scenario) {
            Ok(()) => eprintln!("{scenario}: ok"),
            Err(how) => {
                failures += 1;
                eprintln!("{scenario}: {how}");
            }
        }
    }
    assert_eq!(failures, 0, "{failures} scenario(s) failed");
}

#[test]
fn guest_traps_are_caught_whatever_the_resumption_order() {
    let mut scenarios = vec![];
    for order in [
        "a_traps_while_b_suspended",
        "b_traps_after_a_returned",
        "b_traps_after_a_dropped",
    ] {
        for stores in ["one_store", "two_stores"] {
            for trap in TRAPS {
                scenarios.push(format!("trap:{order}:{stores}:{}", trap.0));
            }
        }
    }
    run_children(scenarios);
}

#[test]
fn a_poller_running_wasm_keeps_its_own_trap_handler() {
    let mut scenarios = vec![];
    for poller in ["sync", "async"] {
        for trap in TRAPS {
            scenarios.push(format!("poller:{poller}:{}", trap.0));
        }
    }
    run_children(scenarios);
}

#[test]
fn a_caught_host_panic_leaves_no_coroutine_behind() {
    run_children(["host_panic_then_sync_call".to_string()]);
}

#[test]
fn a_nested_sync_entry_does_not_suspend() {
    run_children(["nested_sync_entry".to_string()]);
}

#[cfg(all(unix, feature = "experimental-host-interrupt"))]
#[test]
fn an_interrupt_is_judged_against_the_store_that_is_running() {
    run_children(["suspended", "running"].map(|target| format!("interrupt:{target}")));
}

/// Dropping a suspended call unwinds it at once: the host future is dropped,
/// no guest code runs, and the store is free and usable.
#[test]
fn dropping_a_suspended_call_unwinds_it() {
    let g = gates();
    let guest = Guest::new(&g);
    let mut call = guest.run(0, 0);
    expect_pending("call", &mut call);
    assert!(!guest.host_future_dropped[0].get());
    assert!(guest.try_write().is_some(), "lock released at suspension");

    drop(call);
    assert!(guest.host_future_dropped[0].get(), "host future dropped");
    assert_eq!(guest.after(), 0, "no guest code ran after the import");

    g[0].set(true);
    expect_value("next call", &mut guest.run(0, 0), 42);
    assert_eq!(guest.after(), 1);
    guest.into_store();
}

/// A call that had to wait for the store takes it through the lock future
/// that holds its place among the waiters, also when the store is free by the
/// time the call is polled again. That place is given up when the call gets
/// the store: dropping the call later must not clear a waiter that has taken
/// the same place since.
#[test]
fn a_call_that_waited_for_the_store_gives_up_its_place_among_the_waiters() {
    let g = gates();
    g[0].set(true);
    let guest = Guest::new(&g);

    let holder = guest.try_write().expect("store is free");
    let mut call = guest.run(0, 0);
    expect_pending("the call waits for the store", &mut call);
    drop(holder);
    expect_value("call", &mut call, 42);

    let holder = guest.try_write().expect("store is free");
    let wakes = Arc::new(Wakes::default());
    let mut waiter = Box::pin(guest.store.write_lock());
    let poll = waiter
        .as_mut()
        .poll(&mut Context::from_waker(&Waker::from(wakes.clone())));
    assert!(poll.is_pending());
    drop(call);
    drop(holder);
    assert_eq!(wakes.count(), 1, "the waiter is told the store is free");
}

/// Three waiters for a store: B waits, is woken, and finds the store taken
/// again; C starts waiting meanwhile. Both are woken when the store is
/// released, C again when B is done with it, and the store can be taken
/// back out of its async handle afterwards.
#[test]
fn every_waiter_for_the_store_is_woken() {
    let g = gates();
    let guest = Guest::new(&g);
    let lock = |wakes: &Arc<Wakes>| {
        let mut lock = Box::pin(guest.store.write_lock());
        let waker = Waker::from(wakes.clone());
        let poll = lock.as_mut().poll(&mut Context::from_waker(&waker));
        (lock, poll)
    };
    let (wakes_b, wakes_c) = (Arc::new(Wakes::default()), Arc::new(Wakes::default()));

    let a = guest.try_write().expect("the store is free");
    let (mut b, poll) = lock(&wakes_b);
    assert!(poll.is_pending());
    drop(a);
    assert_eq!(wakes_b.count(), 1);

    let a = guest.try_write().expect("the store is free");
    let (mut c, poll) = lock(&wakes_c);
    assert!(poll.is_pending());
    let waker_b = Waker::from(wakes_b.clone());
    assert!(
        b.as_mut()
            .poll(&mut Context::from_waker(&waker_b))
            .is_pending()
    );
    drop(a);
    assert_eq!((wakes_b.count(), wakes_c.count()), (2, 1));

    let Poll::Ready(guard_b) = b.as_mut().poll(&mut Context::from_waker(&waker_b)) else {
        panic!("B should have the store");
    };
    let waker_c = Waker::from(wakes_c.clone());
    assert!(
        c.as_mut()
            .poll(&mut Context::from_waker(&waker_c))
            .is_pending()
    );
    drop(guard_b);
    assert_eq!(wakes_c.count(), 2);
    let Poll::Ready(guard_c) = c.as_mut().poll(&mut Context::from_waker(&waker_c)) else {
        panic!("C should have the store");
    };
    drop((guard_c, b, c));
    guest.into_store();
}

/// A typed `call_async` waits for the store as a reader, `Function::call_async`
/// as a writer. Releasing the store wakes only the writer; when that call is
/// dropped before it is polled again, the typed call is woken in its place.
#[test]
fn a_call_waiting_behind_a_dropped_call_is_woken() {
    let g = gates();
    let guest = Guest::new(&g);
    let typed: TypedFunction<i32, i32> = {
        let guard = guest.try_write().expect("the store is free");
        guest
            .instance
            .exports
            .get_typed_function(&guard, "via_host")
            .unwrap()
    };
    let (wakes_typed, wakes_untyped) = (Arc::new(Wakes::default()), Arc::new(Wakes::default()));
    let waker_typed = Waker::from(wakes_typed.clone());
    let waker_untyped = Waker::from(wakes_untyped.clone());

    let holder = guest.try_write().expect("the store is free");
    let mut typed_call = Box::pin(typed.call_async(&guest.store, 0));
    let poll = typed_call
        .as_mut()
        .poll(&mut Context::from_waker(&waker_typed));
    assert!(poll.is_pending());
    let mut untyped_call = guest.call("via_host", &[0]);
    let poll = untyped_call
        .as_mut()
        .poll(&mut Context::from_waker(&waker_untyped));
    assert!(poll.is_pending());
    drop(holder);
    assert_eq!((wakes_untyped.count(), wakes_typed.count()), (1, 0));

    drop(untyped_call);
    assert_eq!(
        wakes_typed.count(),
        1,
        "the typed call is told the store is free"
    );
    let poll = typed_call
        .as_mut()
        .poll(&mut Context::from_waker(&waker_typed));
    assert!(matches!(poll, Poll::Ready(Ok(7))), "{poll:?}");
    drop(typed_call);
    guest.into_store();
}

/// A call polled from inside the store it runs in, by a host function of
/// another call on that store, cannot have the store and says so. A host
/// function that catches the refusal goes on, and so does its call.
#[test]
fn a_call_polled_from_inside_its_own_store_is_refused() {
    let g = gates();
    let guest = Guest::new(&g);
    let refusal: Rc<RefCell<Option<String>>> = Rc::default();
    guest.on_host({
        let store = guest.store.store();
        let run = guest.instance.exports.get_function("run").unwrap().clone();
        let refusal = refusal.clone();
        move || {
            let args = vec![Value::I32(0), Value::I32(0)];
            let mut call: CallFuture = Box::pin(run.call_async(&store, args));
            let panic =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| poll_once(&mut call)))
                    .expect_err("the poll is refused");
            *refusal.borrow_mut() = panic.downcast_ref::<&str>().map(|s| s.to_string());
            Ok(())
        }
    });
    let mut outer = guest.call("via_host", &[0]);
    expect_value("outer", &mut outer, 7);
    let refusal = refusal.borrow().clone().expect("a panic with a message");
    assert!(
        refusal.contains("cannot be polled recursively"),
        "{refusal}"
    );
    drop(outer);
    expect_value("next call", &mut guest.call("via_host", &[0]), 7);
    assert!(guest.try_write().is_some(), "the store is free");
}

/// A host function running on the coroutine of a call instantiates a module
/// whose start function waits on an async import. The start function
/// suspends the call, as the host function's own calls into Wasm would, and
/// the instance is created once the import is ready.
#[test]
fn a_start_function_that_waits_suspends_the_call_that_instantiates() {
    const START: &str = r#"
    (module
      (import "h" "wait" (func $wait))
      (global (export "started") (mut i32) (i32.const 0))
      (func $start (call $wait) (global.set 0 (i32.const 1)))
      (start $start))
    "#;
    let gate = Rc::new(Cell::new(false));
    let mut store = Store::default();
    let module = Module::new(&store, START).unwrap();
    let wait = Function::new_async(&mut store, FunctionType::new(vec![], vec![]), {
        let gate = gate.clone();
        move |_| {
            let gate = gate.clone();
            async move {
                Gate(gate).await;
                Ok(vec![])
            }
        }
    });
    let env = wasmer::FunctionEnv::new(&mut store, ());
    let created: Arc<std::sync::Mutex<Option<Instance>>> = Arc::default();
    let instantiate =
        Function::new_with_env(&mut store, &env, FunctionType::new(vec![], vec![]), {
            let created = created.clone();
            move |mut env: wasmer::FunctionEnvMut<()>, _: &[Value]| {
                let imports = imports! { "h" => { "wait" => wait.clone() } };
                let instance = Instance::new(&mut env, &module, &imports)
                    .map_err(|e| RuntimeError::new(e.to_string()))?;
                *created.lock().unwrap() = Some(instance);
                Ok(vec![])
            }
        });
    let store = store.into_async();

    let mut call: CallFuture = Box::pin(instantiate.call_async(&store, vec![]));
    expect_pending("instantiation", &mut call);
    assert!(created.lock().unwrap().is_none());
    gate.set(true);
    assert!(matches!(poll_once(&mut call), Poll::Ready(Ok(_))));
    drop(call);
    let Ok(mut store) = store.into_store() else {
        panic!("the store is still shared");
    };
    let instance = created
        .lock()
        .unwrap()
        .take()
        .expect("the instance was created");
    let started = instance.exports.get_global("started").unwrap();
    assert_eq!(started.get(&mut store).unwrap_i32(), 1);
}

/// A host function catches a panic from a guest it called, and goes on using
/// its store. Its borrow of the store, which the guest call paused, is given
/// back while the panic unwinds, as are the borrows of the host functions the
/// panic unwinds through.
mod caught_panics {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use wasmer::{AsyncFunctionEnvMut, FunctionEnv, FunctionEnvMut};

    use super::*;

    const WAT: &str = r#"
    (module
      (import "h" "sync" (func $sync))
      (import "h" "typed" (func $typed))
      (import "h" "async_body" (func $async_body))
      (import "h" "async_future" (func $async_future))
      (import "h" "wait" (func $wait))
      (import "h" "catch" (func $catch (param i32) (result i32)))
      (import "h" "drop_call" (func $drop_call))
      (import "h" "drop_sync" (func $drop_sync))
      (func (export "sync") (call $sync))
      (func (export "typed") (call $typed))
      (func (export "async_body") (call $async_body))
      (func (export "async_future") (call $async_future))
      (func (export "wait") (call $wait))
      (func (export "drop_sync") (call $drop_sync))
      (func (export "catch") (param i32) (result i32) (call $catch (local.get 0)))
      (func (export "drop_call") (call $drop_call)))
    "#;

    /// The host functions that panic, by the export that calls them: a
    /// dynamic one, a typed one, an async one in its body and an async one
    /// in its future. `wait` does not panic: it is pending.
    /// `drop_sync` does not either: it drops the call in `TO_DROP_SYNC`.
    const NESTED: [&str; 6] = [
        "sync",
        "typed",
        "async_body",
        "async_future",
        "wait",
        "drop_sync",
    ];

    thread_local! {
        /// A call for `drop_sync` to drop.
        static TO_DROP_SYNC: RefCell<Option<CallFuture>> = const { RefCell::new(None) };
    }

    #[derive(Default)]
    struct Env {
        nested: Vec<Function>,
        /// How many times `catch` went on after the guest call.
        went_on: i32,
    }

    struct Guest {
        store: Store,
        instance: Instance,
        /// A call for `drop_call` to drop.
        to_drop: Rc<RefCell<Option<CallFuture>>>,
    }

    impl Guest {
        fn new() -> Self {
            let mut store = Store::default();
            let module = Module::new(&store, WAT).unwrap();
            let env = FunctionEnv::new(&mut store, Env::default());
            let unit = FunctionType::new(vec![], vec![]);
            let sync = Function::new(&mut store, unit.clone(), |_| panic!("host function"));
            fn typed_panics() {
                panic!("typed host function")
            }
            let typed = Function::new_typed(&mut store, typed_panics);
            let async_body = Function::new_with_env_async(
                &mut store,
                &env,
                unit.clone(),
                |_: AsyncFunctionEnvMut<Env>, _: &[Value]| -> std::future::Ready<_> {
                    panic!("async host function")
                },
            );
            let async_future = Function::new_with_env_async(
                &mut store,
                &env,
                unit.clone(),
                |_: AsyncFunctionEnvMut<Env>, _: &[Value]| async { panic!("host future") },
            );
            let wait = Function::new_async(&mut store, unit.clone(), |_| async {
                Gate(Rc::new(Cell::new(false))).await;
                Ok(vec![])
            });
            // Calls the export `NESTED[i]` and catches a panic from it. Returns
            // how many times it went on after catching one, or -1 if there was
            // none to catch.
            let catch = Function::new_with_env(
                &mut store,
                &env,
                FunctionType::new(vec![Type::I32], vec![Type::I32]),
                |mut env: FunctionEnvMut<Env>, args: &[Value]| {
                    let nested = env.data().nested[args[0].unwrap_i32() as usize].clone();
                    let result = catch_unwind(AssertUnwindSafe(|| nested.call(&mut env, &[])));
                    let caught = match result {
                        Ok(_) => false,
                        Err(payload) if payload.is::<&str>() => true,
                        // A call that is dropped while suspended is unwound
                        // with a payload of its own, which must go on.
                        Err(payload) => std::panic::resume_unwind(payload),
                    };
                    env.data_mut().went_on += 1;
                    let went_on = env.data().went_on;
                    Ok(vec![Value::I32(if caught { went_on } else { -1 })])
                },
            );
            let to_drop: Rc<RefCell<Option<CallFuture>>> = Rc::default();
            let drop_call = Function::new_async(&mut store, unit.clone(), {
                let to_drop = to_drop.clone();
                move |_| {
                    drop(to_drop.borrow_mut().take());
                    async { Ok(vec![]) }
                }
            });
            let drop_sync = Function::new(&mut store, unit, |_| {
                drop(TO_DROP_SYNC.take());
                Ok(vec![])
            });
            let instance = Instance::new(
                &mut store,
                &module,
                &imports! { "h" => {
                    "sync" => sync,
                    "typed" => typed,
                    "async_body" => async_body,
                    "async_future" => async_future,
                    "wait" => wait,
                    "catch" => catch,
                    "drop_call" => drop_call,
                    "drop_sync" => drop_sync,
                } },
            )
            .unwrap();
            env.as_mut(&mut store).nested = NESTED
                .iter()
                .map(|name| instance.exports.get_function(name).unwrap().clone())
                .collect();
            Self {
                store,
                instance,
                to_drop,
            }
        }

        fn export(&self, name: &str) -> Function {
            self.instance.exports.get_function(name).unwrap().clone()
        }
    }

    fn call(store: &StoreAsync, f: &Function, args: &[i32]) -> CallFuture {
        let args = args.iter().map(|v| Value::I32(*v)).collect();
        Box::pin(f.call_async(store, args))
    }

    #[test]
    fn on_a_call_async_coroutine() {
        for (i, nested) in NESTED[..3].iter().enumerate() {
            let guest = Guest::new();
            let catch = guest.export("catch");
            let store = guest.store.into_async();
            for round in 1..=2 {
                expect_value(nested, &mut call(&store, &catch, &[i as i32]), round);
            }
            assert!(store.into_store().is_ok(), "{nested}");
        }
    }

    /// The future of an async host function is polled by the future of the
    /// call, so its panic reaches whoever polls the call rather than the host
    /// function beneath. The call is unwound when it is dropped, and the store
    /// goes on being usable.
    #[test]
    fn a_host_future_panics_in_the_poller() {
        let guest = Guest::new();
        let catch = guest.export("catch");
        let store = guest.store.into_async();
        let mut panicking = call(&store, &catch, &[3]);
        expect_host_panic(&mut panicking);
        drop(panicking);
        expect_value("after the panic", &mut call(&store, &catch, &[0]), 1);
        assert!(store.into_store().is_ok());
    }

    /// Nothing changes for a synchronous call, whose host functions borrow
    /// entries of their own, and whose host futures are polled inside the
    /// import call: the guest cannot be suspended.
    #[test]
    fn on_a_synchronous_call() {
        for (i, nested) in NESTED[..4].iter().enumerate() {
            let mut guest = Guest::new();
            let catch = guest.export("catch");
            for round in 1..=2 {
                let values = catch
                    .call(&mut guest.store, &[Value::I32(i as i32)])
                    .unwrap();
                assert_eq!(values[0].unwrap_i32(), round, "{nested}");
            }
        }
    }

    /// A call that is suspended under a host function holding a paused
    /// borrow, and is dropped by another call on the same store, is unwound
    /// without changing the borrows of the call that drops it.
    #[test]
    fn a_dropped_call_leaves_the_borrows_of_the_call_that_drops_it() {
        let guest = Guest::new();
        let (catch, drop_call) = (guest.export("catch"), guest.export("drop_call"));
        let to_drop = guest.to_drop.clone();
        let store = guest.store.into_async();
        let mut suspended = call(&store, &catch, &[4]);
        expect_pending("suspended", &mut suspended);
        *to_drop.borrow_mut() = Some(suspended);
        match poll_once(&mut call(&store, &drop_call, &[])) {
            Poll::Ready(Ok(_)) => {}
            other => panic!("the call that drops it: {other:?}"),
        }
        assert!(to_drop.borrow().is_none());
        assert!(store.into_store().is_ok());
    }

    /// The same when the call that drops it runs on another store: the
    /// dropped call's borrows belong to an entry that is not the active one,
    /// and the borrow its host function gives back while it is unwound is
    /// not taken from the call that drops it. That call's host function
    /// holds a borrow of its own store while it drops the suspended call.
    #[test]
    fn a_dropped_call_leaves_the_borrows_of_a_call_on_another_store() {
        let (a, b) = (Guest::new(), Guest::new());
        let catch_a = a.export("catch");
        let catch_b = b.export("catch");
        let (store_a, store_b) = (a.store.into_async(), b.store.into_async());
        let mut suspended = call(&store_a, &catch_a, &[4]);
        expect_pending("suspended", &mut suspended);
        TO_DROP_SYNC.set(Some(suspended));
        // `catch` returns -1: `drop_sync` did not panic.
        expect_value(
            "the call that drops it",
            &mut call(&store_b, &catch_b, &[5]),
            -1,
        );
        assert!(TO_DROP_SYNC.with_borrow(Option::is_none));
        assert!(store_a.into_store().is_ok());
        assert!(store_b.into_store().is_ok());
    }
}
