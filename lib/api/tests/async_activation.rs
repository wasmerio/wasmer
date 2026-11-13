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
    StoreAsyncWriteLock, Type, Value, imports,
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

/// A host future is polled for the first time on the coroutine of the call,
/// inside the import call, and the coroutine suspends only if the future is
/// pending. That poll has to behave as the poller's own polls do: the future
/// can lock the store, keep it, panic, and call into guests.
mod host_future_first_poll {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use wasmer::{AsyncFunctionEnvHandleMut, AsyncFunctionEnvMut, FunctionEnv, FunctionEnvMut};

    use super::*;

    const WAT: &str = r#"
    (module
      (import "h" "f" (func $f (result i32)))
      (import "h" "wait" (func $wait))
      (import "h" "nested_f" (func $nested_f (result i32)))
      (func (export "f") (result i32) (call $f))
      (func (export "wait") (call $wait))
      (func (export "f_then_wait") (result i32)
        (local i32)
        (local.set 0 (call $f))
        (call $wait)
        (local.get 0))
      ;; `f` called through a host function.
      (func (export "nested_f") (result i32) (call $nested_f)))
    "#;

    #[derive(Default)]
    struct Env {
        value: i32,
        f: Option<Function>,
    }

    type HostResult = Pin<Box<dyn Future<Output = Result<Vec<Value>, RuntimeError>>>>;

    /// What the host future of the `wait` import saw.
    #[derive(Default)]
    struct Wait {
        open: Cell<bool>,
        polls: Cell<usize>,
        waker: RefCell<Option<Waker>>,
    }

    struct Guest {
        store: StoreAsync,
        instance: Instance,
        wait: Rc<Wait>,
        #[cfg(all(unix, feature = "experimental-host-interrupt"))]
        interrupter: wasmer::Interrupter,
    }

    impl Guest {
        /// `f` is the async import of that name. `wait` is pending until it
        /// is opened. `nested_f` is a synchronous host function that calls
        /// the export `f`.
        fn new(f: impl Fn(AsyncFunctionEnvMut<Env>) -> HostResult + 'static) -> Self {
            let mut store = Store::default();
            let module = Module::new(&store, WAT).unwrap();
            let env = FunctionEnv::new(&mut store, Env::default());
            let returns_i32 = FunctionType::new(vec![], vec![Type::I32]);
            let f = Function::new_with_env_async(
                &mut store,
                &env,
                returns_i32.clone(),
                move |env, _| f(env),
            );
            let wait = Rc::new(Wait::default());
            let wait_import = Function::new_async(&mut store, FunctionType::new(vec![], vec![]), {
                let wait = wait.clone();
                move |_| {
                    let wait = wait.clone();
                    std::future::poll_fn(move |cx| {
                        wait.polls.set(wait.polls.get() + 1);
                        if wait.open.get() {
                            return Poll::Ready(Ok(vec![]));
                        }
                        *wait.waker.borrow_mut() = Some(cx.waker().clone());
                        Poll::Pending
                    })
                }
            });
            let nested_f = Function::new_with_env(
                &mut store,
                &env,
                returns_i32,
                |mut env: FunctionEnvMut<Env>, _: &[Value]| {
                    let f = env.data().f.clone().unwrap();
                    f.call(&mut env, &[]).map(Vec::from)
                },
            );
            let instance = Instance::new(
                &mut store,
                &module,
                &imports! { "h" => { "f" => f, "wait" => wait_import, "nested_f" => nested_f } },
            )
            .unwrap();
            env.as_mut(&mut store).f = Some(instance.exports.get_function("f").unwrap().clone());
            Self {
                #[cfg(all(unix, feature = "experimental-host-interrupt"))]
                interrupter: store.interrupter(),
                store: store.into_async(),
                instance,
                wait,
            }
        }

        fn export(&self, name: &str) -> Function {
            self.instance.exports.get_function(name).unwrap().clone()
        }

        fn call(&self, name: &str) -> CallFuture {
            Box::pin(self.export(name).call_async(&self.store, vec![]))
        }

        fn store_is_free(&self) -> bool {
            Box::pin(self.store.write_lock())
                .as_mut()
                .poll(&mut Context::from_waker(&futures::task::noop_waker()))
                .is_ready()
        }
    }

    /// Adds 5 to the environment's value and returns it.
    async fn add_five(env: AsyncFunctionEnvMut<Env>) -> Result<Vec<Value>, RuntimeError> {
        let mut env = env.write().await;
        env.data_mut().value += 5;
        Ok(vec![Value::I32(env.data_mut().value)])
    }

    fn ready(value: i32) -> HostResult {
        Box::pin(async move { Ok(vec![Value::I32(value)]) })
    }

    /// Where on the stack the caller is, give or take a frame.
    #[inline(never)]
    fn stack_position() -> usize {
        let local = 0u8;
        std::hint::black_box(&local) as *const u8 as usize
    }

    #[test]
    fn it_happens_on_the_stack_of_the_call() {
        let in_body = Rc::new(Cell::new(0usize));
        let in_poll = Rc::new(Cell::new(0usize));
        let guest = Guest::new({
            let (in_body, in_poll) = (in_body.clone(), in_poll.clone());
            move |_| {
                // The body of an async import runs on the coroutine.
                in_body.set(stack_position());
                let in_poll = in_poll.clone();
                Box::pin(std::future::poll_fn(move |_| {
                    in_poll.set(stack_position());
                    Poll::Ready(Ok(vec![Value::I32(1)]))
                }))
            }
        });
        expect_value("call", &mut guest.call("f"), 1);
        let distance = in_body.get().abs_diff(in_poll.get());
        assert!(distance < 64 * 1024, "polled {distance} bytes away");
        assert!(in_poll.get().abs_diff(stack_position()) > 64 * 1024);
    }

    /// A pending future has been polled once when the call returns pending,
    /// with the task context the call was polled with.
    #[test]
    fn it_is_given_the_context_of_the_poller() {
        let guest = Guest::new(|_| ready(1));
        let wakes = Arc::new(Wakes::default());
        let waker = Waker::from(wakes.clone());
        let mut call = guest.call("wait");
        let poll = call.as_mut().poll(&mut Context::from_waker(&waker));
        assert!(poll.is_pending());
        assert_eq!(guest.wait.polls.get(), 1);
        // Giving the store up woke nobody: nothing waits for it.
        assert_eq!(wakes.count(), 0);
        guest.wait.waker.take().unwrap().wake();
        assert_eq!(wakes.count(), 1);
        guest.wait.open.set(true);
        assert!(matches!(poll_once(&mut call), Poll::Ready(Ok(_))));
        assert_eq!(guest.wait.polls.get(), 2);
    }

    /// A call polled from inside another call's import has a task context of
    /// its own. The outer call's next import gets the outer one again.
    #[test]
    fn it_is_given_the_context_of_its_own_poller_after_a_nested_poll() {
        let inner = Guest::new(|_| ready(1));
        let inner_wakes = Arc::new(Wakes::default());
        let inner_wait = inner.wait.clone();
        let outer = Guest::new({
            let inner_wakes = inner_wakes.clone();
            move |_| {
                // Leaked, so that it would still be there if the outer call
                // went on using it.
                let waker: &'static Waker = Box::leak(Box::new(Waker::from(inner_wakes.clone())));
                let cx = Box::leak(Box::new(Context::from_waker(waker)));
                let mut call = inner.call("wait");
                assert!(call.as_mut().poll(cx).is_pending());
                inner.wait.open.set(true);
                assert!(matches!(call.as_mut().poll(cx), Poll::Ready(Ok(_))));
                ready(2)
            }
        });
        let outer_wakes = Arc::new(Wakes::default());
        let waker = Waker::from(outer_wakes.clone());
        let mut call = outer.call("f_then_wait");
        let poll = call.as_mut().poll(&mut Context::from_waker(&waker));
        assert!(poll.is_pending());
        inner_wait.waker.take().unwrap().wake();
        outer.wait.waker.take().unwrap().wake();
        assert_eq!((inner_wakes.count(), outer_wakes.count()), (1, 1));
        outer.wait.open.set(true);
        expect_value("outer", &mut call, 2);
    }

    #[test]
    fn it_may_lock_the_store_and_be_ready() {
        let guest = Guest::new(|env| Box::pin(add_five(env)));
        expect_value("first", &mut guest.call("f"), 5);
        expect_value("second", &mut guest.call("f"), 10);
        // Also when a host function of the call holds a borrow of the store
        // across the guest call that reaches the import.
        expect_value("nested", &mut guest.call("nested_f"), 15);
        assert!(guest.store.into_store().is_ok());
    }

    #[test]
    fn it_may_keep_the_store_while_pending() {
        let gate = Rc::new(Cell::new(false));
        let guest = Guest::new({
            let gate = gate.clone();
            move |env| {
                let gate = gate.clone();
                Box::pin(async move {
                    let mut env = env.write().await;
                    Gate(gate).await;
                    env.data_mut().value += 5;
                    Ok(vec![Value::I32(env.data_mut().value)])
                })
            }
        });
        let mut call = guest.call("f");
        expect_pending("call", &mut call);
        assert!(!guest.store_is_free(), "the host future has the store");
        gate.set(true);
        expect_value("call", &mut call, 5);
        drop(call);
        assert!(guest.store.into_store().is_ok());
    }

    /// The guest cannot go on without the store, so the call waits for it.
    #[test]
    fn it_may_be_ready_and_leave_the_store_locked() {
        let kept: Rc<RefCell<Option<AsyncFunctionEnvHandleMut<Env>>>> = Rc::default();
        let guest = Guest::new({
            let kept = kept.clone();
            move |env| {
                let kept = kept.clone();
                Box::pin(async move {
                    *kept.borrow_mut() = Some(env.write().await);
                    Ok(vec![Value::I32(9)])
                })
            }
        });
        let mut call = guest.call("f");
        expect_pending("the call waits for the store", &mut call);
        expect_pending("and keeps waiting", &mut call);
        kept.borrow_mut().take();
        expect_value("call", &mut call, 9);
        drop(call);

        // A call dropped while it waits for the store is unwound like any
        // other suspended call.
        let mut call = guest.call("f");
        expect_pending("the call waits for the store", &mut call);
        drop(call);
        kept.borrow_mut().take();
        assert!(guest.store.into_store().is_ok());
    }

    #[test]
    fn it_may_panic() {
        let guest = Guest::new(|_| Box::pin(async { panic!("host future panicked") }));
        let mut call = guest.call("f");
        let caught = catch_unwind(AssertUnwindSafe(|| poll_once(&mut call)));
        let message = *caught.unwrap_err().downcast::<&str>().unwrap();
        assert_eq!(message, "host future panicked");
        drop(call);
        assert!(guest.store_is_free());
        expect_host_panic(&mut guest.call("nested_f"));
        assert!(guest.store.into_store().is_ok());
    }

    /// An interrupt of the store that arrives while the call is in host code
    /// is noticed when the call next leaves its coroutine, also when it
    /// leaves with a host future that it has polled already.
    #[cfg(all(unix, feature = "experimental-host-interrupt"))]
    #[test]
    fn it_may_be_pending_in_a_store_that_was_interrupted() {
        let interrupter: Rc<RefCell<Option<wasmer::Interrupter>>> = Rc::default();
        let guest = Guest::new({
            let interrupter = interrupter.clone();
            move |_| {
                interrupter.borrow().as_ref().unwrap().interrupt();
                Box::pin(std::future::pending())
            }
        });
        *interrupter.borrow_mut() = Some(guest.interrupter.clone());
        let mut call = guest.call("f");
        expect_trap("call", &mut call, TrapCode::HostInterrupt);
    }

    /// While the future is polled, no coroutine is current: guest code the
    /// future calls into cannot suspend, not even when it runs in a store
    /// that an outer `call_async` holds and could otherwise suspend with.
    ///
    /// Here the outer call polls the inner one from the body of an import,
    /// and the inner call's host future enters the outer call's store, which
    /// is the thread's current one while the inner call has lent its own.
    #[test]
    fn it_cannot_suspend_a_call_from_inside_the_poll() {
        let outer_wait: Rc<RefCell<Option<Function>>> = Rc::default();
        let inner = Guest::new({
            let outer_wait = outer_wait.clone();
            move |_| {
                let outer_wait = outer_wait.borrow().clone().unwrap();
                Box::pin(async move {
                    let result = Store::with_current(|store| outer_wait.call(store, &[]))
                        .expect("the outer call's store is current");
                    assert_trap(
                        "pending import",
                        &result.unwrap_err(),
                        TrapCode::YieldOutsideAsyncContext,
                    );
                    Ok(vec![Value::I32(3)])
                })
            }
        });
        let outer = Guest::new(move |_| {
            let value = match poll_once(&mut inner.call("f")) {
                Poll::Ready(Ok(values)) => values[0].unwrap_i32(),
                other => panic!("inner call: {other:?}"),
            };
            ready(value + 4)
        });
        *outer_wait.borrow_mut() = Some(outer.export("wait"));
        expect_value("outer", &mut outer.call("f"), 7);
        assert!(outer.store.into_store().is_ok());
    }
}
