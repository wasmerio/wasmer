wasm_test!(
    test_libc_clock_function,
    "libc-clock-function",
    stdout = "Clock works."
);
wasm_test!(
    test_libc_getpass,
    "libc-getpass",
    stdout = "getpass test - requires interactive terminal"
);
wasm_test!(
    test_mmap_anon,
    "mmap-anon",
    stdout = "mmap anon memory works"
);
wasm_test!(
    test_variadic_args,
    "variadic-args",
    stdout = "Printing 5, 6, 0, 42"
);
