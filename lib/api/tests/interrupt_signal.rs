#![cfg(all(unix, feature = "experimental-host-interrupt"))]

//! Tests for the signal used to interrupt running WASM code: selecting it,
//! and how a delivery nobody asked for is handled.
//!
//! Both the selection and the installed signal handlers are process-global
//! state that can only be set up once, so every scenario runs in its own
//! subprocess: the test binary re-executes itself with
//! [`SCENARIO_ENV_VAR`] set, which makes [`scenario_entrypoint`] run the
//! requested scenario.

use std::{
    env, mem,
    os::unix::process::ExitStatusExt,
    process::Command,
    ptr,
    sync::{
        Arc, Barrier, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use anyhow::Result;
use wasmer::{
    DEFAULT_INTERRUPT_SIGNAL, Instance, InterruptSignal, Module, SetInterruptSignalError, Store,
    imports, interrupt_signal, set_interrupt_signal,
};
use wasmer_vm::TrapCode;

const SCENARIO_ENV_VAR: &str = "WASMER_INTERRUPT_SIGNAL_TEST_SCENARIO";

const INFINITE_LOOP_WAT: &str = r#"
    (module
      (func (export "infinite")
        loop
          br 0
        end
      )
    )"#;

#[test]
fn default_signal_is_sigusr1() {
    run_scenario("default_signal_is_sigusr1");
}

#[test]
fn sigusr2_can_be_selected() {
    run_scenario("sigusr2_can_be_selected");
}

#[test]
fn signal_cant_be_selected_after_handler_installation() {
    run_scenario("signal_cant_be_selected_after_handler_installation");
}

/// A delivery of the interrupt signal that Wasmer didn't request means
/// something else in the process is using it. That can't be handled here and
/// can't be reported by panicking either — unwinding out of a signal handler
/// is undefined behaviour — so the handler explains itself on stderr and
/// aborts.
#[test]
fn unrequested_interrupt_signal_kills_the_process() {
    let output = run_scenario_expecting_death("unrequested_interrupt_signal");

    assert_eq!(
        output.status.signal(),
        Some(libc::SIGABRT),
        "expected the handler to abort, got {}\n--- stderr ---\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("delivered without an interrupt being requested"),
        "the handler didn't explain itself\n--- stderr ---\n{stderr}"
    );
    assert!(
        stderr.contains("set_interrupt_signal"),
        "the message should point at the fix\n--- stderr ---\n{stderr}"
    );
    assert!(
        !stderr.contains("panicked at"),
        "the handler panicked instead of aborting\n--- stderr ---\n{stderr}"
    );
}

/// The entrypoint the subprocesses spawned by [`run_scenario`] run. Does
/// nothing when the test binary is run normally.
#[test]
fn scenario_entrypoint() {
    let Ok(scenario) = env::var(SCENARIO_ENV_VAR) else {
        return;
    };

    match scenario.as_str() {
        "default_signal_is_sigusr1" => default_signal_is_sigusr1_scenario(),
        "sigusr2_can_be_selected" => sigusr2_can_be_selected_scenario(),
        "signal_cant_be_selected_after_handler_installation" => {
            signal_cant_be_selected_after_handler_installation_scenario()
        }
        "unrequested_interrupt_signal" => unrequested_interrupt_signal_scenario(),
        other => panic!("unknown scenario: {other}"),
    }
    .unwrap();
}

/// Runs a scenario that is expected to take the process down, and hands the
/// caller its output to inspect.
fn run_scenario_expecting_death(scenario: &str) -> std::process::Output {
    let output = spawn_scenario(scenario);

    assert!(
        !output.status.success(),
        "scenario `{scenario}` was expected to terminate the process, but it succeeded\
         \n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    output
}

fn spawn_scenario(scenario: &str) -> std::process::Output {
    let test_binary = env::current_exe().expect("failed to locate the test binary");
    Command::new(&test_binary)
        .args([
            "scenario_entrypoint",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(SCENARIO_ENV_VAR, scenario)
        .output()
        .expect("failed to run the scenario subprocess")
}

fn run_scenario(scenario: &str) {
    let output = spawn_scenario(scenario);

    assert!(
        output.status.success(),
        "scenario `{scenario}` failed with {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn default_signal_is_sigusr1_scenario() -> Result<()> {
    assert_eq!(DEFAULT_INTERRUPT_SIGNAL, InterruptSignal::Sigusr1);
    assert_eq!(interrupt_signal(), InterruptSignal::Sigusr1);

    // Explicitly selecting the default is fine, and doesn't change anything.
    set_interrupt_signal(InterruptSignal::Sigusr1)?;
    assert_eq!(interrupt_signal(), InterruptSignal::Sigusr1);

    assert_running_wasm_is_interrupted()
}

fn sigusr2_can_be_selected_scenario() -> Result<()> {
    set_interrupt_signal(InterruptSignal::Sigusr2)?;
    assert_eq!(interrupt_signal(), InterruptSignal::Sigusr2);

    // Repeating the same selection is a no-op...
    set_interrupt_signal(InterruptSignal::Sigusr2)?;

    // ... but switching to another signal is an error, even before the
    // handler is installed.
    assert!(matches!(
        set_interrupt_signal(InterruptSignal::Sigusr1),
        Err(SetInterruptSignalError::AlreadySet {
            current: InterruptSignal::Sigusr2,
            requested: InterruptSignal::Sigusr1
        })
    ));
    assert_eq!(interrupt_signal(), InterruptSignal::Sigusr2);

    // Nothing should touch SIGUSR1 anymore, so an embedder is free to use it
    // for its own purposes.
    install_sigusr1_observer();
    assert_running_wasm_is_interrupted()?;
    assert!(
        !sigusr1_was_observed(),
        "interrupting WASM code raised SIGUSR1 even though SIGUSR2 was selected"
    );
    // Make sure the observer is actually still installed, i.e. the assertion
    // above didn't pass just because Wasmer replaced the handler.
    unsafe { libc::raise(libc::SIGUSR1) };
    assert!(
        sigusr1_was_observed(),
        "the SIGUSR1 handler installed by the embedder was overwritten"
    );

    // Now that the handler is installed, the selection is locked in.
    assert!(matches!(
        set_interrupt_signal(InterruptSignal::Sigusr1),
        Err(SetInterruptSignalError::HandlerAlreadyInstalled {
            current: InterruptSignal::Sigusr2,
            requested: InterruptSignal::Sigusr1
        })
    ));
    // ... though repeating the current selection still succeeds.
    set_interrupt_signal(InterruptSignal::Sigusr2)?;

    Ok(())
}

fn signal_cant_be_selected_after_handler_installation_scenario() -> Result<()> {
    // Building a store installs the signal handlers.
    let _store = Store::default();

    assert!(matches!(
        set_interrupt_signal(InterruptSignal::Sigusr2),
        Err(SetInterruptSignalError::HandlerAlreadyInstalled {
            current: InterruptSignal::Sigusr1,
            requested: InterruptSignal::Sigusr2
        })
    ));
    assert_eq!(interrupt_signal(), InterruptSignal::Sigusr1);

    // The signal that *was* installed keeps working.
    assert_running_wasm_is_interrupted()
}

fn unrequested_interrupt_signal_scenario() -> Result<()> {
    // Building a store installs the handler for the interrupt signal.
    let _store = Store::default();

    // Nobody asked for an interrupt, so this stands in for an unrelated part
    // of the process using the same signal. The handler should print an
    // explanation and exit before `raise` returns.
    unsafe { libc::raise(libc::SIGUSR1) };

    panic!("the process should have been terminated by the interrupt handler");
}

/// Runs an infinite WASM loop on a worker thread and interrupts it through
/// the currently selected signal, asserting that the code actually traps.
fn assert_running_wasm_is_interrupted() -> Result<()> {
    let barrier = Arc::new(Barrier::new(2));
    let interrupter_slot = Arc::new(Mutex::new(None));

    let worker = thread::spawn({
        let barrier = barrier.clone();
        let interrupter_slot = interrupter_slot.clone();
        move || {
            let wasm = wat::parse_str(INFINITE_LOOP_WAT)?;

            let mut store = Store::default();
            interrupter_slot
                .lock()
                .unwrap()
                .replace(store.interrupter());
            let module = Module::new(&store, &wasm)?;
            let instance = Instance::new(&mut store, &module, &imports! {})?;
            let f = instance
                .exports
                .get_typed_function::<(), ()>(&store, "infinite")?;

            barrier.wait();
            anyhow::Ok(f.call(&mut store))
        }
    });

    barrier.wait();
    // Make absolutely sure the function is running WASM when we raise the signal
    thread::sleep(Duration::from_millis(500));

    interrupter_slot
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .interrupt();
    let result = worker.join().unwrap().unwrap().unwrap_err();
    assert_eq!(result.to_trap().unwrap(), TrapCode::HostInterrupt);

    Ok(())
}

static SIGUSR1_OBSERVED: AtomicBool = AtomicBool::new(false);

extern "C" fn sigusr1_observer(_signum: libc::c_int) {
    SIGUSR1_OBSERVED.store(true, Ordering::SeqCst);
}

/// Installs a SIGUSR1 handler standing in for an embedder that uses the
/// signal for its own purposes.
fn install_sigusr1_observer() {
    unsafe {
        let mut action: libc::sigaction = mem::zeroed();
        action.sa_sigaction = sigusr1_observer as *const () as usize;
        action.sa_flags = 0;
        libc::sigemptyset(&mut action.sa_mask);
        assert_eq!(
            libc::sigaction(libc::SIGUSR1, &action, ptr::null_mut()),
            0,
            "failed to install the SIGUSR1 observer: {}",
            std::io::Error::last_os_error()
        );
    }
}

fn sigusr1_was_observed() -> bool {
    SIGUSR1_OBSERVED.swap(false, Ordering::SeqCst)
}
