#[test]
fn test_test_atomic() {
    assert_emscripten_output!(
        "../../emtests/test_atomic.wasm",
        "test_atomic",
        vec![],
        "../../emtests/test_atomic.out"
    );
}
