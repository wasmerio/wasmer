// With `v8-default` (the Windows test lane) `Store::default()` is a V8 store,
// which these native scenarios cannot run on.
#![cfg(all(
    feature = "unsafe-cothread",
    not(feature = "v8-default"),
    not(target_arch = "wasm32")
))]
//! Coroutines that an embedder switches itself (`unsafe-cothread`), each
//! running a call on a store of its own, suspended inside a host function and
//! resumed under the store's `CoroutineStoreGuard`, in any order.
//!
//! What the runtime keeps per thread for a running call (the store context
//! entries and the trap handler the signal handler consults) has to follow
//! the coroutine that is running. A regression there ends with the process
//! killed by a signal, so the scenarios run in a child process (this test
//! binary, re-executed) and are reported as failed tests.

use std::{
    io::Read,
    process::{Command, Stdio},
    ptr,
    time::{Duration, Instant},
};

use corosensei::{Coroutine, CoroutineResult, Yielder};
use wasmer::{
    Function, FunctionEnv, FunctionEnvMut, Instance, Module, RuntimeError, Store, TypedFunction,
    imports,
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

/// The yielder of the coroutine the store's call runs on.
struct Suspender(*const Suspend);
// Safety: only used on the thread that runs the coroutine.
unsafe impl Send for Suspender {}

/// A call to `run` on a store of its own, on a coroutine the test switches.
struct Cothread {
    // Dropped after the coroutine, whose frames point into it.
    coroutine: Option<Coroutine<(), (), Result<i32, RuntimeError>>>,
    store: Box<Store>,
}

impl Cothread {
    fn new(how: i32) -> Self {
        let mut store = Box::new(Store::default());
        let module = Module::new(&*store, WAT).unwrap();
        let env = FunctionEnv::new(&mut *store, Suspender(ptr::null()));
        let suspend =
            Function::new_typed_with_env(&mut *store, &env, |env: FunctionEnvMut<Suspender>| {
                // Safety: set by the coroutine before it calls in.
                unsafe { &*env.data().0 }.suspend(());
            });
        let imports = imports! { "h" => { "suspend" => suspend } };
        let instance = Instance::new(&mut *store, &module, &imports).unwrap();
        let run: TypedFunction<i32, i32> =
            instance.exports.get_typed_function(&*store, "run").unwrap();
        let store_ptr: *mut Store = &mut *store;
        let coroutine = Coroutine::new(move |suspend: &Suspend, ()| {
            // Safety: the store is boxed and outlives the coroutine. While
            // the coroutine runs, the test touches it only to resume it.
            let store = unsafe { &mut *store_ptr };
            env.as_mut(store).0 = suspend;
            run.call(store, how)
        });
        Self {
            coroutine: Some(coroutine),
            store,
        }
    }

    /// Resumes the coroutine under its store's guard: `None` if it
    /// suspended again, or what the call returned.
    fn resume(&mut self) -> Option<Result<i32, RuntimeError>> {
        let store: *mut Store = &mut *self.store;
        // Safety: held for exactly the duration of the resume, on the thread
        // the coroutine runs on.
        let _guard = unsafe { (*store).coroutine_store_guard() };
        match self.coroutine.as_mut().unwrap().resume(()) {
            CoroutineResult::Yield(()) => None,
            CoroutineResult::Return(result) => Some(result),
        }
    }

    /// Drops the suspended coroutine, which resumes it to unwind it, under
    /// its store's guard.
    fn drop_suspended(&mut self) {
        let store: *mut Store = &mut *self.store;
        // Safety: as for `resume`.
        let _guard = unsafe { (*store).coroutine_store_guard() };
        drop(self.coroutine.take());
    }
}

fn ending(name: &str) -> (i32, Option<TrapCode>) {
    let (_, how, trap) = *ENDINGS.iter().find(|e| e.0 == name).unwrap();
    (how, trap)
}

fn expect_suspended(what: &str, cothread: &mut Cothread) {
    assert!(cothread.resume().is_none(), "{what} should suspend");
}

fn expect_ending(what: &str, cothread: &mut Cothread, trap: Option<TrapCode>) {
    let result = cothread
        .resume()
        .unwrap_or_else(|| panic!("{what} suspended"));
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

/// One coroutine, suspended in a host function and resumed: the guard of
/// each resume finds the store off the thread.
fn suspend_and_resume(ending_name: &str) {
    let (how, trap) = ending(ending_name);
    let mut a = Cothread::new(how);
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
    let mut a = Cothread::new(how);
    let mut b = Cothread::new(how);
    expect_suspended("a", &mut a);
    expect_suspended("b", &mut b);
    expect_ending("a", &mut a, trap);
    expect_ending("b", &mut b, trap);
    assert!(no_store_context_left());
}

/// A and B both suspend; A is dropped, which unwinds it, and B goes on.
fn dropped_while_another_is_suspended(ending_name: &str) {
    let (how, trap) = ending(ending_name);
    let mut a = Cothread::new(how);
    let mut b = Cothread::new(how);
    expect_suspended("a", &mut a);
    expect_suspended("b", &mut b);
    a.drop_suspended();
    assert!(no_store_context_left());
    expect_ending("b", &mut b, trap);
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
fn a_coroutine_resumed_under_its_guard_finds_its_store() {
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
