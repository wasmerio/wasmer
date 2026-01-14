mod wasixcc_test_utils;
use crate::wasixcc_test_utils::WasixccTest;

// macOS is currently disabled, because cranelift does not
// support exception handling on that platform yet.
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_simple_switching() {
    let test = WasixccTest::new(file!(), "simple_switching");
    test.compile().unwrap();
    test.run().unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_switching_with_main() {
    let test = WasixccTest::new(file!(), "switching_with_main");
    test.compile().unwrap();
    test.run().unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_switching_to_a_deleted_context() {
    let test = WasixccTest::new(file!(), "switching_to_a_deleted_context");
    test.compile().unwrap();
    test.run().unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_switching_threads() {
    let test = WasixccTest::new(file!(), "switching_in_threads");
    test.compile().unwrap();
    test.run().unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_multiple_contexts() {
    let test = WasixccTest::new(file!(), "multiple_contexts");
    test.compile().unwrap();
    test.run().unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_error_handling() {
    let test = WasixccTest::new(file!(), "error_handling");
    test.compile().unwrap();
    test.run().unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_nested_switches() {
    let test = WasixccTest::new(file!(), "nested_switches");
    test.compile().unwrap();
    test.run().unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_contexts_with_mutexes() {
    let test = WasixccTest::new(file!(), "contexts_with_mutexes");
    test.compile().unwrap();
    test.run().unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_contexts_with_env_vars() {
    let test = WasixccTest::new(file!(), "contexts_with_env_vars");
    test.compile().unwrap();
    test.run().unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_contexts_with_signals() {
    let test = WasixccTest::new(file!(), "contexts_with_signals");
    test.compile().unwrap();
    test.run().unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_contexts_with_timers() {
    let test = WasixccTest::new(file!(), "contexts_with_timers");
    test.compile().unwrap();
    test.run().unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_contexts_with_pipes() {
    let test = WasixccTest::new(file!(), "contexts_with_pipes");
    test.compile().unwrap();
    test.run().unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_pending_file_operations() {
    let test = WasixccTest::new(file!(), "pending_file_operations");
    test.compile().unwrap();
    test.run().unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_recursive_host_calls() {
    let test = WasixccTest::new(file!(), "recursive_host_calls");
    test.compile().unwrap();
    test.run().unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_malloc_during_switch() {
    let test = WasixccTest::new(file!(), "malloc_during_switch");
    test.compile().unwrap();
    test.run().unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_nested_host_call_switch() {
    let test = WasixccTest::new(file!(), "nested_host_call_switch");
    test.compile().unwrap();
    test.run().unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_switch_to_never_resumed() {
    let test = WasixccTest::new(file!(), "switch_to_never_resumed");
    test.compile().unwrap();
    test.run().unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_three_way_recursion() {
    let test = WasixccTest::new(file!(), "three_way_recursion");
    test.compile().unwrap();
    test.run().unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_contexts_with_setjmp() {
    let test = WasixccTest::new(file!(), "contexts_with_setjmp");
    test.compile().unwrap();
    test.run().unwrap();
}
