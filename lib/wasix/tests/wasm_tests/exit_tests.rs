wasm_test!(test_exit_zero, "exit-zero");
wasm_test!(test_exit_nonzero, "exit-nonzero", should_fail);
wasm_test!(test_exit_zero_in_thread, "exit-zero-in-thread");
wasm_test!(
    #[ignore = "flaky test (#6538)"]
    test_exit_nonzero_in_thread,
    "exit-nonzero-in-thread",
    should_fail
);
wasm_test!(test_exit_zero_in_closure_call, "exit-zero-in-closure-call");
wasm_test!(
    test_exit_nonzero_in_closure_call,
    "exit-nonzero-in-closure-call",
    should_fail
);
wasm_test!(
    test_exit_zero_in_closure_call_thread,
    "exit-zero-in-closure-call-thread"
);
wasm_test!(
    #[ignore = "flaky test (#6538)"]
    test_exit_nonzero_in_closure_call_thread,
    "exit-nonzero-in-closure-call-thread",
    should_fail
);
wasm_test!(
    test_exit_zero_in_call_dynamic_thread,
    "exit-zero-in-call-dynamic-thread"
);
wasm_test!(
    #[ignore = "flaky test (#6538)"]
    test_exit_nonzero_in_call_dynamic_thread,
    "exit-nonzero-in-call-dynamic-thread",
    should_fail
);

const SIGABRT_EXIT_CODE: i32 = 134;
wasm_test!(
    #[ignore = "flaky test (#6538)"]
    test_abort_in_thread,
    "abort-in-thread",
    exit_code = SIGABRT_EXIT_CODE
);
