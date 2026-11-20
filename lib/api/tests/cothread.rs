// With `v8-default` (the Windows test lane) `Store::default()` is a V8 store,
// which these native scenarios cannot run on.
#![cfg(all(
    feature = "unsafe-cothread",
    not(feature = "v8-default"),
    not(target_arch = "wasm32")
))]
//! Coroutines that an embedder switches itself (`unsafe-cothread`), each
//! running a call that suspends inside a host function, resumed through
//! `Cothread` in any order, dropped or forgotten while suspended.
//!
//! What the runtime keeps per thread for a running call (the store context
//! entries and the trap handler the signal handler consults) has to follow
//! the coroutine that is running. A regression there ends with the process
//! killed by a signal, so the scenarios run in a child process (this test
//! binary, re-executed) and are reported as failed tests.

use std::{
    cell::RefCell,
    io::Read,
    process::{Command, Stdio},
    ptr,
    time::{Duration, Instant},
};

use corosensei::{Coroutine, CoroutineResult, Yielder};
use wasmer::{
    AsStoreMut, Cothread, Function, FunctionEnv, FunctionEnvMut, Instance, Module, RuntimeError,
    Store, TypedFunction, imports,
};
use wasmer_vm::TrapCode;

const WAT: &str = r#"
(module
  (import "h" "suspend" (func $suspend))
  (memory 1)
  (func $rec (param i32) (result i32)
    (i32.add (call $rec (i32.add (local.get 0) (i32.const 1))) (i32.const 1)))
  ;; Suspends the coroutine it runs on, then traps as asked or returns 42.
  (func (export "run") (param $how i32) (result i32)
    (call $suspend)
    (if (i32.eq (local.get $how) (i32.const 1))
      (then (drop (i32.load (i32.const 0x7ffffff0)))))
    (if (i32.eq (local.get $how) (i32.const 2))
      (then (drop (i32.div_u (i32.const 1) (i32.const 0)))))
    (if (i32.eq (local.get $how) (i32.const 3))
      (then (drop (call $rec (i32.const 0)))))
    (if (i32.eq (local.get $how) (i32.const 4))
      (then unreachable))
    (i32.const 42)))
"#;

/// What `run` does after it is resumed, the argument that asks for it, and
/// the trap that ends it, if any. Whether a trap is raised through a signal
/// depends on the target and the compiler.
const ENDINGS: [(&str, i32, Option<TrapCode>); 5] = [
    ("return", 0, None),
    ("oob", 1, Some(TrapCode::HeapAccessOutOfBounds)),
    ("div0", 2, Some(TrapCode::IntegerDivisionByZero)),
    ("overflow", 3, Some(TrapCode::StackOverflow)),
    ("unreachable", 4, Some(TrapCode::UnreachableCodeReached)),
];

type Suspend = Yielder<(), ()>;
type Call = Coroutine<(), (), Result<i32, RuntimeError>>;

/// The yielder of the coroutine the store's call runs on.
struct Suspender(*const Suspend);
// Safety: only used on the thread that runs the coroutine.
unsafe impl Send for Suspender {}

thread_local! {
    /// What the next `suspend` import runs before it suspends its coroutine.
    static BEFORE_SUSPEND: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
}

/// A store with the guest instantiated, and a call to `run` on it, on a
/// coroutine the test switches.
struct Guest {
    // Dropped before the store, which the coroutine's frames point into.
    cothread: Option<Cothread<Call>>,
    run: TypedFunction<i32, i32>,
    env: FunctionEnv<Suspender>,
    store: Box<Store>,
}

impl Guest {
    fn new(how: i32) -> Self {
        let mut store = Box::new(Store::default());
        let module = Module::new(&*store, WAT).unwrap();
        let env = FunctionEnv::new(&mut *store, Suspender(ptr::null()));
        let suspend =
            Function::new_typed_with_env(&mut *store, &env, |env: FunctionEnvMut<Suspender>| {
                if let Some(before) = BEFORE_SUSPEND.with(|cell| cell.borrow_mut().take()) {
                    before();
                }
                // Safety: set by the coroutine before it calls in.
                unsafe { &*env.data().0 }.suspend(());
            });
        let imports = imports! { "h" => { "suspend" => suspend } };
        let instance = Instance::new(&mut *store, &module, &imports).unwrap();
        let run = instance.exports.get_typed_function(&*store, "run").unwrap();
        let mut guest = Self {
            cothread: None,
            run,
            env,
            store,
        };
        guest.start(how);
        guest
    }

    /// Starts a call to `run` on a new coroutine, dropping the previous one.
    fn start(&mut self, how: i32) {
        self.cothread = None;
        let store_ptr: *mut Store = &mut *self.store;
        let (env, run) = (self.env.clone(), self.run.clone());
        let coroutine = Coroutine::new(move |suspend: &Suspend, ()| {
            // Safety: the store is boxed and outlives the coroutine. While
            // the coroutine is suspended, the test uses the store only to
            // start a call in place of a forgotten one.
            let store = unsafe { &mut *store_ptr };
            env.as_mut(store).0 = suspend;
            run.call(store, how)
        });
        // Safety: the `Cothread` owns the `Coroutine`.
        self.cothread = Some(unsafe { Cothread::new(coroutine) });
    }

    /// Resumes the coroutine: `None` if it suspended again, or what the call
    /// returned.
    fn resume(&mut self) -> Option<Result<i32, RuntimeError>> {
        let cothread = self.cothread.as_mut().expect("a call was started");
        // Safety: resumes the coroutine, on the thread that made it.
        match unsafe { cothread.resume(|coroutine| coroutine.resume(())) } {
            CoroutineResult::Yield(()) => None,
            CoroutineResult::Return(result) => Some(result),
        }
    }
}

fn ending(name: &str) -> (i32, Option<TrapCode>) {
    let (_, how, trap) = *ENDINGS.iter().find(|e| e.0 == name).unwrap();
    (how, trap)
}

fn expect_suspended(what: &str, guest: &mut Guest) {
    assert!(guest.resume().is_none(), "{what} should suspend");
}

fn expect_ending(what: &str, guest: &mut Guest, trap: Option<TrapCode>) {
    let result = guest.resume().unwrap_or_else(|| panic!("{what} suspended"));
    match (result, trap) {
        (Ok(value), None) => assert_eq!(value, 42, "{what}"),
        (Err(e), Some(code)) => assert_eq!(e.clone().to_trap(), Some(code), "{what}: {e}"),
        (result, _) => panic!("{what}: unexpected {result:?}"),
    }
}

/// Whether the thread is left without a store context.
fn no_store_context_left() -> bool {
    Store::with_current(|_| ()).is_none()
}

/// One coroutine, suspended in a host function and resumed: its store is
/// off the thread in between.
fn suspend_and_resume(ending_name: &str) {
    let (how, trap) = ending(ending_name);
    let mut a = Guest::new(how);
    expect_suspended("a", &mut a);
    assert!(
        no_store_context_left(),
        "a's store context left on the thread"
    );
    expect_ending("a", &mut a, trap);
    assert!(no_store_context_left());
}

/// A and B enter Wasm in that order and both suspend; A is resumed first,
/// so the thread switches back to a call that is not the last one to have
/// entered Wasm.
fn interleaved(ending_name: &str) {
    let (how, trap) = ending(ending_name);
    let mut a = Guest::new(how);
    let mut b = Guest::new(how);
    expect_suspended("a", &mut a);
    expect_suspended("b", &mut b);
    expect_ending("a", &mut a, trap);
    expect_ending("b", &mut b, trap);
    assert!(no_store_context_left());
}

/// A and B both suspend; A is dropped, which unwinds it, and B goes on.
fn dropped_while_another_is_suspended(ending_name: &str) {
    let (how, trap) = ending(ending_name);
    let mut a = Guest::new(how);
    let mut b = Guest::new(how);
    expect_suspended("a", &mut a);
    expect_suspended("b", &mut b);
    drop(a);
    assert!(no_store_context_left());
    expect_ending("b", &mut b, trap);
    assert!(no_store_context_left());
}

/// A suspends and is forgotten; a new call on A's store, on a new coroutine,
/// runs with nothing of the forgotten one's state.
fn forgotten_then_a_new_call_on_its_store(ending_name: &str) {
    let (how, trap) = ending(ending_name);
    let mut a = Guest::new(how);
    expect_suspended("a", &mut a);
    std::mem::forget(a.cothread.take());
    a.start(how);
    expect_suspended("a's new call", &mut a);
    expect_ending("a's new call", &mut a, trap);
    assert!(no_store_context_left());
}

/// A suspends; B's host function drops it, or resumes it to its end, before
/// B suspends in turn. The thread switches from B's call to A's and back
/// while B is inside a host function.
fn from_inside_another_host_function(what: &'static str, ending_name: &str) {
    let (how, trap) = ending(ending_name);
    let mut a = Guest::new(how);
    let mut b = Guest::new(how);
    expect_suspended("a", &mut a);
    let before: Box<dyn FnOnce()> = match what {
        "drop" => Box::new(move || drop(a)),
        "resume" => Box::new(move || {
            let mut a = a;
            expect_ending("a", &mut a, trap);
        }),
        other => panic!("unknown action {other}"),
    };
    BEFORE_SUSPEND.with(|cell| *cell.borrow_mut() = Some(before));
    expect_suspended("b", &mut b);
    assert!(BEFORE_SUSPEND.with(|cell| cell.borrow().is_none()));
    assert!(no_store_context_left());
    expect_ending("b", &mut b, trap);
    assert!(no_store_context_left());
}

/// A suspends; a host function of a call on A's own store drops it. A cannot
/// be unwound there, where its store is in use: it is forgotten, with the
/// interrupt registration of its call, and the store goes on.
fn dropped_where_its_store_is_in_use(ending_name: &str) {
    thread_local! {
        static TO_DROP: RefCell<Option<Cothread<Call>>> = const { RefCell::new(None) };
    }
    let (how, trap) = ending(ending_name);
    let mut a = Guest::new(how);
    expect_suspended("a", &mut a);
    TO_DROP.set(a.cothread.take());
    let dropper = Function::new_typed(&mut *a.store, || drop(TO_DROP.take()));
    dropper.call(&mut *a.store, &[]).unwrap();
    assert!(TO_DROP.with_borrow(Option::is_none));
    assert!(no_store_context_left());
    // Nothing is registered for interrupts any more: one now would keep the
    // store interrupted, and fail its next call.
    #[cfg(all(unix, feature = "experimental-host-interrupt"))]
    assert!(matches!(
        wasmer_vm::interrupt_registry::interrupt(a.store.id()),
        Err(wasmer_vm::interrupt_registry::InterruptError::StoreNotRunning)
    ));
    a.start(how);
    expect_suspended("a's new call", &mut a);
    expect_ending("a's new call", &mut a, trap);
    assert!(no_store_context_left());
}

/// A coroutine resumed from a host function of a synchronous call into a
/// plain store, lent that store, suspends inside a call on it: the call
/// beneath goes on, its host functions with their own borrow of the store,
/// and returns; the coroutine is then resumed at the top of the thread, to
/// its end.
fn lent_by_a_synchronous_call(ending_name: &str) {
    struct Outer {
        run: TypedFunction<i32, i32>,
        suspender: FunctionEnv<Suspender>,
        how: i32,
        went_on: i32,
    }
    thread_local! {
        static LENT: RefCell<Option<Cothread<Call>>> = const { RefCell::new(None) };
    }
    let (how, trap) = ending(ending_name);
    let mut guest = Guest::new(how);
    // Never resumed: the coroutine of the test is made below.
    guest.cothread = None;
    let store = &mut *guest.store;
    let env = FunctionEnv::new(
        store,
        Outer {
            run: guest.run.clone(),
            suspender: guest.env.clone(),
            how,
            went_on: 0,
        },
    );
    let resume = Function::new_typed_with_env(store, &env, |mut env: FunctionEnvMut<Outer>| {
        let (run, suspender, how) = {
            let outer = env.data();
            (outer.run.clone(), outer.suspender.clone(), outer.how)
        };
        // Safety: the store outlives the coroutine, which uses it while
        // nothing else does.
        let mut store: wasmer::StoreMut<'static> =
            unsafe { std::mem::transmute(env.as_store_mut()) };
        let coroutine = Coroutine::new(move |suspend: &Suspend, ()| {
            suspender.as_mut(&mut store).0 = suspend;
            run.call(&mut store, how)
        });
        // Safety: the `Cothread` owns the `Coroutine`.
        let mut cothread = unsafe { Cothread::new(coroutine) };
        // Safety: resumes the coroutine, on the thread that made it.
        match unsafe { cothread.resume(|coroutine| coroutine.resume(())) } {
            CoroutineResult::Yield(()) => LENT.set(Some(cothread)),
            CoroutineResult::Return(result) => panic!("the coroutine returned {result:?}"),
        }
        env.data_mut().went_on += 1;
    });
    let after =
        Function::new_typed_with_env(store, &env, |env: FunctionEnvMut<Outer>| env.data().went_on);
    let module = Module::new(
        &*store,
        r#"(module
          (import "h" "resume" (func $resume))
          (import "h" "after" (func $after (result i32)))
          (func (export "outer") (result i32) (call $resume) (call $after)))"#,
    )
    .unwrap();
    let imports = imports! { "h" => { "resume" => resume, "after" => after } };
    let instance = Instance::new(store, &module, &imports).unwrap();
    let outer: TypedFunction<(), i32> =
        instance.exports.get_typed_function(store, "outer").unwrap();
    assert_eq!(outer.call(store).unwrap(), 1, "the call beneath went on");
    assert!(no_store_context_left());

    let mut cothread = LENT.take().expect("the coroutine suspended");
    // Safety: resumes the coroutine, on the thread that made it.
    let result = match unsafe { cothread.resume(|coroutine| coroutine.resume(())) } {
        CoroutineResult::Return(result) => result,
        CoroutineResult::Yield(()) => panic!("the coroutine suspended again"),
    };
    match (result, trap) {
        (Ok(value), None) => assert_eq!(value, 42),
        (Err(e), Some(code)) => assert_eq!(e.clone().to_trap(), Some(code), "{e}"),
        (result, _) => panic!("unexpected {result:?}"),
    }
    drop(cothread);
    assert!(no_store_context_left());
}

/// A coroutine resumed from a host function of a call that `call_async`
/// runs: it runs on its own stack, not on the call's coroutine.
#[cfg(feature = "experimental-async")]
mod inside_call_async {
    use std::{
        cell::Cell,
        future::Future,
        pin::{Pin, pin},
        task::{Context, Poll, Waker},
    };

    use wasmer::{AsStoreMut, FunctionType, StoreAsync, StoreMut};

    use super::*;

    const WAT: &str = r#"
    (module
      (import "h" "resume" (func $resume))
      (import "h" "wait" (func $wait))
      (import "h" "suspend" (func $suspend))
      ;; Starts the coroutine, then waits in an async import itself.
      (func (export "outer") (result i32)
        (call $resume)
        (call $wait)
        (i32.const 1))
      (func (export "waits") (result i32)
        (call $wait)
        (i32.const 42))
      (func (export "suspends") (result i32)
        (call $suspend)
        (i32.const 42)))
    "#;

    /// Pending once.
    struct YieldOnce(bool);
    impl Future for YieldOnce {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            if std::mem::replace(&mut self.0, true) {
                return Poll::Ready(());
            }
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }

    /// Polls `future` until it is ready: what it waits for is ready when it
    /// is polled again.
    fn drive<F: Future>(future: F) -> F::Output {
        let mut future = pin!(future);
        let mut cx = Context::from_waker(Waker::noop());
        loop {
            if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
                return output;
            }
        }
    }

    /// What the coroutine does.
    #[derive(Clone)]
    enum Run {
        /// Calls an export of the store of the call that resumes it.
        Export(TypedFunction<(), i32>),
        /// Calls, with `call_async`, `suspends` of the store in [`SECOND`].
        Second,
    }

    thread_local! {
        static SUSPEND: Cell<*const Suspend> = const { Cell::new(ptr::null()) };
        static COTHREAD: RefCell<Option<Cothread<Call>>> = const { RefCell::new(None) };
        static RETURNED: RefCell<Option<Result<i32, RuntimeError>>> = const { RefCell::new(None) };
        static SECOND: RefCell<Option<(StoreAsync, TypedFunction<(), i32>)>> =
            const { RefCell::new(None) };
    }

    fn suspend_import(store: &mut Store) -> Function {
        Function::new_typed(store, || {
            // Safety: set by the coroutine, which this suspends.
            unsafe { &*SUSPEND.get() }.suspend(())
        })
    }

    /// A store whose `outer` starts a coroutine that does `run`, given the
    /// name of an export or none, and keeps it if it suspends.
    fn guest(run: Option<&str>) -> (StoreAsync, Function) {
        struct Runs(Option<Run>);
        let mut store = Store::default();
        let env = FunctionEnv::new(&mut store, Runs(None));
        let resume =
            Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Runs>| {
                let run = env.data().0.clone().unwrap_or(Run::Second);
                // Safety: the store outlives the coroutine, which uses it
                // while nothing else does.
                let mut store: StoreMut<'static> =
                    unsafe { std::mem::transmute(env.as_store_mut()) };
                let coroutine = Coroutine::new(move |suspend: &Suspend, ()| {
                    SUSPEND.set(suspend);
                    match run {
                        Run::Export(export) => export.call(&mut store),
                        Run::Second => SECOND.with_borrow(|second| {
                            let (store, suspends) = second.as_ref().unwrap();
                            drive(suspends.call_async(store))
                        }),
                    }
                });
                // Safety: the `Cothread` owns the `Coroutine`.
                let mut cothread = unsafe { Cothread::new(coroutine) };
                // Safety: resumes the coroutine, on the thread that made it.
                match unsafe { cothread.resume(|coroutine| coroutine.resume(())) } {
                    CoroutineResult::Return(result) => RETURNED.set(Some(result)),
                    CoroutineResult::Yield(()) => COTHREAD.set(Some(cothread)),
                }
            });
        let wait = Function::new_async(&mut store, FunctionType::new([], []), |_| async {
            YieldOnce(false).await;
            Ok(vec![])
        });
        let module = Module::new(&store, WAT).unwrap();
        let imports = imports! { "h" => {
            "resume" => resume,
            "wait" => wait,
            "suspend" => suspend_import(&mut store),
        } };
        let instance = Instance::new(&mut store, &module, &imports).unwrap();
        let get = |name: &str| instance.exports.get_typed_function(&store, name).unwrap();
        env.as_mut(&mut store).0 = run.map(|name| Run::Export(get(name)));
        let outer = instance.exports.get_function("outer").unwrap().clone();
        (store.into_async(), outer)
    }

    /// Runs `outer` with `call_async`: it returns once the coroutine has
    /// returned or suspended, and its own async import has waited.
    fn run_outer(store: &StoreAsync, outer: &Function) {
        let results = drive(outer.call_async(store, vec![])).unwrap();
        assert_eq!(results[0].unwrap_i32(), 1);
    }

    /// Resumes the coroutine `outer` left suspended, at the top of the
    /// thread, to its end.
    fn finish_coroutine() -> Result<i32, RuntimeError> {
        let mut cothread = COTHREAD.take().expect("the coroutine suspended");
        // Safety: resumes the coroutine, on the thread that made it.
        match unsafe { cothread.resume(|coroutine| coroutine.resume(())) } {
            CoroutineResult::Return(result) => result,
            CoroutineResult::Yield(()) => panic!("the coroutine suspended again"),
        }
    }

    pub(super) fn run(scenario: &str) {
        match scenario {
            // An async import the coroutine calls cannot suspend the call
            // that resumed it, from the coroutine's stack: it waits, and so
            // fails as in a synchronous call.
            "async_import" => {
                let (store, outer) = guest(Some("waits"));
                run_outer(&store, &outer);
                let error = RETURNED.take().expect("returned").unwrap_err();
                assert_eq!(error.to_trap(), Some(TrapCode::YieldOutsideAsyncContext));
            }
            // The coroutine suspends inside a call on the store the call that
            // resumed it holds: what its frames borrow goes with it, and the
            // call goes on, suspends and returns without it. The coroutine is
            // then resumed to its end.
            "suspended" => {
                let (store, outer) = guest(Some("suspends"));
                run_outer(&store, &outer);
                assert!(RETURNED.with_borrow(Option::is_none));
                assert_eq!(finish_coroutine().unwrap(), 42);
                drop(store);
            }
            // The coroutine suspends inside a `call_async` coroutine it runs
            // itself, on another store: the call that resumed it is on its
            // own coroutine again when it goes on, and suspends there.
            "nested_call_async" => {
                let mut second = Store::default();
                let module = Module::new(
                    &second,
                    r#"(module
                      (import "h" "suspend" (func $suspend))
                      (func (export "suspends") (result i32)
                        (call $suspend)
                        (i32.const 42)))"#,
                )
                .unwrap();
                let imports = imports! { "h" => { "suspend" => suspend_import(&mut second) } };
                let instance = Instance::new(&mut second, &module, &imports).unwrap();
                let suspends = instance
                    .exports
                    .get_typed_function(&second, "suspends")
                    .unwrap();
                SECOND.set(Some((second.into_async(), suspends)));
                let (store, outer) = guest(None);
                run_outer(&store, &outer);
                assert!(RETURNED.with_borrow(Option::is_none));
                assert_eq!(finish_coroutine().unwrap(), 42);
                drop(SECOND.take());
            }
            other => panic!("unknown scenario {other}"),
        }
        assert!(no_store_context_left());
    }
}

const SCENARIO_ENV: &str = "WASMER_COTHREAD_SCENARIO";
const SCENARIO_COMPLETED: &str = "scenario completed:";
const SCENARIO_TIMEOUT: Duration = Duration::from_secs(60);

/// Runs the scenario named by the environment; does nothing otherwise.
#[test]
fn child_scenario() {
    let Ok(scenario) = std::env::var(SCENARIO_ENV) else {
        return;
    };
    let (kind, ending) = scenario.split_once(':').unwrap();
    match kind {
        "resume" => suspend_and_resume(ending),
        "interleaved" => interleaved(ending),
        "dropped" => dropped_while_another_is_suspended(ending),
        "forgotten" => forgotten_then_a_new_call_on_its_store(ending),
        "drop_inside" => from_inside_another_host_function("drop", ending),
        "resume_inside" => from_inside_another_host_function("resume", ending),
        "dropped_in_use" => dropped_where_its_store_is_in_use(ending),
        "lent" => lent_by_a_synchronous_call(ending),
        #[cfg(feature = "experimental-async")]
        "inside_call_async" => inside_call_async::run(ending),
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

fn run_children(kind: &str) {
    let mut failures = 0;
    for (ending, _, _) in ENDINGS {
        let scenario = format!("{kind}:{ending}");
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
fn a_resumed_coroutine_finds_its_store() {
    run_children("resume");
}

#[test]
fn guest_traps_are_caught_whatever_the_resumption_order() {
    run_children("interleaved");
}

#[test]
fn a_suspended_coroutine_can_be_dropped_while_another_is_suspended() {
    run_children("dropped");
}

#[test]
fn a_forgotten_coroutine_leaves_nothing_for_the_next_call_on_its_store() {
    run_children("forgotten");
}

#[test]
fn a_coroutine_can_be_dropped_from_another_ones_host_function() {
    run_children("drop_inside");
}

#[test]
fn a_coroutine_can_be_resumed_from_another_ones_host_function() {
    run_children("resume_inside");
}

#[test]
fn a_coroutine_dropped_where_its_store_is_in_use_is_forgotten() {
    run_children("dropped_in_use");
}

#[test]
fn a_coroutine_lent_a_plain_store_by_a_synchronous_call_goes_on_without_it() {
    run_children("lent");
}

/// A coroutine resumed from a host function of a call that `call_async`
/// runs is not on that call's coroutine: an async import it calls does not
/// suspend that call, from the coroutine's stack; a call of the coroutine on
/// that call's store borrows it through an entry of its own, which goes with
/// the coroutine when it suspends; and a `call_async` coroutine it suspends
/// in goes with it too.
#[cfg(feature = "experimental-async")]
#[test]
fn a_coroutine_resumed_inside_call_async_runs_on_its_own() {
    let mut failures = Vec::new();
    for scenario in ["async_import", "suspended", "nested_call_async"] {
        let scenario = format!("inside_call_async:{scenario}");
        match run_child(&scenario) {
            Ok(()) => eprintln!("{scenario}: ok"),
            Err(how) => {
                eprintln!("{scenario}: {how}");
                failures.push(scenario);
            }
        }
    }
    assert!(failures.is_empty(), "failed: {failures:?}");
}
