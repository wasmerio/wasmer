#[test]
fn test_test_float32_precise() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_float32_precise.wasm",
        "test_float32_precise",
        vec![],
        "../emscripten_resources/emtests/test_float32_precise.out"
    );
}
