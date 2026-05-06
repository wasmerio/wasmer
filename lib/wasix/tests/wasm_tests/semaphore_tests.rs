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

wasm_test!(
    test_semaphore_unlink_nullptr_exits,
    "semaphore-unlink-nullptr-exits",
    should_fail
);
