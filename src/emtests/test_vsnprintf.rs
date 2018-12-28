#[test]
fn test_test_vsnprintf() {
    assert_emscripten_output!(
        "../../emtests/test_vsnprintf.wasm",
        "test_vsnprintf",
        vec![],
        "../../emtests/test_vsnprintf.out"
    );
}
