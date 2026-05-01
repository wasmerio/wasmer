#![cfg(all(unix, not(feature = "js")))]

wasm_test!(test_simple_shared_lib, "simple-shared-lib");
wasm_test!(test_errno_in_shared_lib, "errno-in-shared-lib");
wasm_test!(
    test_simple_exceptions_in_shared_lib,
    "simple-exceptions-in-shared-lib"
);
