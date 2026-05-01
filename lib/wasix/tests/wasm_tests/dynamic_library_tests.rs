use super::{run_build_script, run_wasm_with_result};

wasm_test!(
    test_simple_dynamic_lib,
    "simple-dynamic-lib",
    stdout = "The shared library returned: 42"
);
wasm_test!(
    test_cpp_dynamic_lib,
    "cpp-dynamic-lib",
    stdout = "Hello world from C++"
);
wasm_test!(
    #[ignore = "Known issue - side module destructors don't run on dlclose yet"]
    test_dlclose_executes_destructors,
    "dlclose-executes-destructors",
    stdout = "abcdef"
);
wasm_test!(test_dlopen_exports, "dlopen-exports");
wasm_test!(test_dylink_needed, "dylink-needed");
wasm_test!(
    #[ignore = "Currently broken - wasm-ld does not support recursive linking yet"]
    test_recursive_shared_lib,
    "recursive-shared-lib",
    stdout = "The shared library returned: 42"
);

#[test]
fn test_duplicate_dynamic_lib() {
    let wasm = run_build_script(file!(), "duplicate-dynamic-lib").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let output = String::from_utf8_lossy(&result.stdout);
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "Module A returned: A");
    assert_eq!(lines[1], "Module B returned: B");
}
