#[test]
fn test_test_float32_precise() {
    assert_emscripten_output!(
        "../../emtests/test_float32_precise.wasm",
        "test_float32_precise",
        vec![],
        "../../emtests/test_float32_precise.out"
    );
}
