#[test]
fn test_test_i64_umul() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_i64_umul.wasm",
        "test_i64_umul",
        vec![],
        "../emscripten_resources/emtests/test_i64_umul.out"
    );
}
