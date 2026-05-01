use super::{run_build_script, run_wasm_with_result};

wasm_test!(test_semaphore_named, "semaphore-named");
wasm_test!(test_semaphore_unnamed, "semaphore-unnamed");
wasm_test!(
    test_semaphore_open_without_create,
    "semaphore-open-without-create"
);
wasm_test!(
    test_semaphore_open_invalid_names,
    "semaphore-open-invalid-names"
);
wasm_test!(
    test_semaphore_same_name_no_create_on_second,
    "semaphore-same-name-no-create-on-second"
);
wasm_test!(
    test_semaphore_same_name_twice_with_excl,
    "semaphore-same-name-twice-with-excl"
);
wasm_test!(
    test_semaphore_same_name_twice_without_excl,
    "semaphore-same-name-twice-without-excl"
);
wasm_test!(test_semaphore_unlink_named, "semaphore-unlink-named");
wasm_test!(
    test_semaphore_unlink_nonexistent,
    "semaphore-unlink-nonexistent"
);

// sem_unlink(NULL) must produce a non-zero exit code (assertion / segfault).
// Checked via run_wasm_with_result because the exit code may come from a trap
// rather than an explicit exit(), and we want to verify it's `Some` and non-zero.
#[test]
fn test_semaphore_unlink_nullptr_exits() {
    let wasm_path = run_build_script(file!(), "semaphore-unlink-nullptr-exits").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    let result = run_wasm_with_result(&wasm_path, test_dir).unwrap();
    assert!(result.exit_code.is_some(), "Expected an exit code");
    assert_ne!(result.exit_code.unwrap(), 0, "Expected non-zero exit code");
}
