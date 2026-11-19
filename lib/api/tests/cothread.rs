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
    Cothread, Function, FunctionEnv, FunctionEnvMut, Instance, Module, RuntimeError, Store,
    TypedFunction, imports,
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
/// be unwound there, where its store is in use: it is forgotten, and the
/// store goes on.
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
    a.start(how);
    expect_suspended("a's new call", &mut a);
    expect_ending("a's new call", &mut a, trap);
    assert!(no_store_context_left());
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
