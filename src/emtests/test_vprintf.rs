#[test]
fn test_test_vprintf() {
    assert_emscripten_output!(
        "../../emtests/test_vprintf.wasm",
        "test_vprintf",
        vec![],
        "../../emtests/test_vprintf.out"
    );
}
