#![cfg(not(target_os = "windows"))]

wasm_test!(simple_exceptions, "simple-exceptions");
wasm_test!(simple_exceptions_with_lto, "simple-exceptions-with-lto");

// #6244
// wasm_test!(simple_exceptions_with_shared_lib_in_callstack, "simple-exceptions-with-shared-lib-in-callstack");

wasm_test!(
    simple_exceptions_with_shared_lib_present,
    "simple-exceptions-with-shared-lib-present"
);
wasm_test!(
    exceptions_catchall_in_shared_lib,
    "exceptions-catchall-in-shared-lib"
);
wasm_test!(
    exceptions_in_main_and_shared,
    "exceptions-in-main-and-shared"
);

// Grid tests for exceptions-across-modules with different combinations
// of static/shared linking for thrower and catcher modules

wasm_test!(
    static_thrower_static_catcher,
    "static-thrower-static-catcher"
);
wasm_test!(
    static_thrower_shared_catcher,
    "static-thrower-shared-catcher"
);

// #6244
// wasm_test!(shared_thrower_static_catcher, "shared-thrower-static-catcher");

wasm_test!(
    shared_thrower_shared_catcher,
    "shared-thrower-shared-catcher"
);
wasm_test!(
    static_thrower_via_shared_proxy_static_catcher,
    "static-thrower-via-shared-proxy-static-catcher"
);
wasm_test!(nested_exceptions, "nested-exceptions");
