#[test]
#[ignore]
fn test_test_lower_intrinsics() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_lower_intrinsics.wasm",
        "test_lower_intrinsics",
        vec![],
        "../emscripten_resources/emtests/test_lower_intrinsics.out"
    );
}
