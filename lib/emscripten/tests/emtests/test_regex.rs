#[test]
fn test_test_regex() {
    assert_emscripten_output!(
        "../../emtests/test_regex.wasm",
        "test_regex",
        vec![],
        "../../emtests/test_regex.out"
    );
}
