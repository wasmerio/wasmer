#[test]
#[ignore]
fn test_test_lower_intrinsics() {
    assert_emscripten_output!(
        "../../emtests/test_lower_intrinsics.wasm",
        "test_lower_intrinsics",
        vec![],
        "../../emtests/test_lower_intrinsics.out"
    );
}
