#[test]
fn test_test_negative_zero() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_negative_zero.wasm",
        "test_negative_zero",
        vec![],
        "../emscripten_resources/emtests/test_negative_zero.out"
    );
}
