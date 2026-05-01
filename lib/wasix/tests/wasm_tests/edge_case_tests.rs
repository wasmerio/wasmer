wasm_test!(
    test_weak_symbol_defined,
    "weak-symbol-defined",
    stdout = "other_func returned 42"
);
wasm_test!(
    test_weak_symbol_undefined,
    "weak-symbol-undefined",
    stdout = "other_func is not defined, but the program still compiled"
);
wasm_test!(
    test_extern_variable,
    "extern-variable",
    stdout = "error number: 444"
);
wasm_test!(
    test_funky_problem,
    "funky-problem",
    stdout = ".Nothing weird happened"
);
wasm_test!(
    test_indirect_call_to_own_function_in_module,
    "indirect-call-to-own-function-in-module",
    stdout = "called"
);
wasm_test!(
    test_llvm_caching_problem,
    "llvm-caching-problem",
    stdout = "The dynamic library returned: 42"
);
