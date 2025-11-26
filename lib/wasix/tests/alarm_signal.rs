// Test for alarm signal functionality in WASIX
// This test verifies that SIGALRM signals fire correctly during thread_sleep

use wasmer::Module;
use wasmer_types::ModuleHash;
use wasmer_wasix::{
    WasiError,
    runners::wasi::{RuntimeOrEngine, WasiRunner},
};

mod sys {
    #[test]
    fn test_alarm_signal() {
        super::test_alarm_signal();
    }
}

/// Tests that SIGALRM fires during thread_sleep.
/// 
/// This test creates a WAT module that:
/// 1. Registers a signal handler using callback_signal
/// 2. Sets up SIGALRM to fire after 100ms using proc_raise_interval
/// 3. Calls thread_sleep for 2 seconds
/// 
/// Expected behavior:
/// - The alarm should fire after 100ms, calling the signal handler
/// - The signal handler calls proc_exit(0) to indicate success
/// 
/// Bug behavior (what this test is meant to catch):
/// - The alarm never fires
/// - thread_sleep completes after 2 seconds
/// - The program exits with code 1 (failure)
fn test_alarm_signal() {
    #[cfg(not(target_arch = "wasm32"))]
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    #[cfg(not(target_arch = "wasm32"))]
    let handle = runtime.handle().clone();
    #[cfg(not(target_arch = "wasm32"))]
    let _guard = handle.enter();

    let engine = wasmer::Engine::default();
    
    // The WAT module that tests alarm signals
    let wat = r#"
    (module
      ;; Import the WASIX syscalls we need
      (import "wasix_32v1" "proc_raise_interval" (func $proc_raise_interval (param i32 i64 i32) (result i32)))
      (import "wasix_32v1" "callback_signal" (func $callback_signal (param i32 i32)))
      (import "wasix_32v1" "thread_sleep" (func $thread_sleep (param i64) (result i32)))
      (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))

      ;; Memory for storing the signal handler name
      (memory (export "memory") 1)
      
      ;; Store the signal handler function name "signal_handler" at address 0
      (data (i32.const 0) "signal_handler")

      ;; Signal handler function - will be called when SIGALRM fires
      ;; This function must be exported so callback_signal can find it
      (func $signal_handler (export "signal_handler") (param $sig i32)
        ;; Exit with code 0 (success - alarm worked!)
        (call $proc_exit (i32.const 0))
      )

      ;; Main function
      (func $_start (export "_start")
        ;; Register the signal handler
        ;; callback_signal(name_ptr=0, name_len=14) - "signal_handler" is 14 chars
        (call $callback_signal (i32.const 0) (i32.const 14))

        ;; Set up SIGALRM (signal 14) to fire after 100ms, no repeat
        ;; proc_raise_interval(sig=14, interval_ms=100, repeat=0)
        (drop (call $proc_raise_interval 
          (i32.const 14)    ;; SIGALRM = 14
          (i64.const 100)   ;; 100 milliseconds
          (i32.const 0)     ;; repeat = false
        ))

        ;; Sleep for 2 seconds (2000000000 nanoseconds)
        ;; This should be interrupted by the alarm after 100ms
        (drop (call $thread_sleep (i64.const 2000000000)))

        ;; If we get here, the alarm didn't fire!
        ;; Exit with code 1 (failure)
        (call $proc_exit (i32.const 1))
      )
    )
    "#;
    
    let module = Module::new(&engine, wat).unwrap();

    let runner = WasiRunner::new();

    let result = runner.run_wasm(
        RuntimeOrEngine::Engine(engine),
        "alarm-test",
        module,
        ModuleHash::random(),
    );

    // The test expects exit code 0 (signal handler was called)
    // If exit code is 1, the alarm didn't fire (bug)
    match result {
        Ok(()) => {
            // Success! The alarm fired and the signal handler exited cleanly
        }
        Err(err) => {
            // Check if this is an exit error
            let exit_code = err.chain().find_map(|e| {
                if let Some(WasiError::Exit(code)) = e.downcast_ref::<WasiError>() {
                    Some(*code)
                } else {
                    None
                }
            });
            
            match exit_code {
                Some(code) if code.raw() == 0 => {
                    // Success! Exit code 0 means the signal handler was called
                }
                Some(code) if code.raw() == 1 => {
                    panic!("ALARM SIGNAL BUG: The alarm did not fire! The thread_sleep completed without the signal handler being called. Exit code: {}", code.raw());
                }
                Some(code) => {
                    panic!("Unexpected exit code: {}", code.raw());
                }
                None => {
                    panic!("Unexpected error (not a WasiError::Exit): {:?}", err);
                }
            }
        }
    }
}
