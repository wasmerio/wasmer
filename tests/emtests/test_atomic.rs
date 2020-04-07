#[test]
fn test_test_atomic() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_atomic.wasm",
        "test_atomic",
        vec![],
        "../emscripten_resources/emtests/test_atomic.out"
    );
}
