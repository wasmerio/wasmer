wasm_test!(test_simple_longjmp, "simple-longjmp", stdout = "abc");
wasm_test!(
    test_longjmp_in_library,
    "longjmp-in-library",
    stdout = "abc"
);
wasm_test!(
    test_longjmp_across_libraries,
    "longjmp-across-libraries",
    stdout = "abc"
);
wasm_test!(
    test_longjmp_across_libraries_buffer_in_lib,
    "longjmp-across-libraries-buffer-in-lib",
    stdout = "abc"
);
