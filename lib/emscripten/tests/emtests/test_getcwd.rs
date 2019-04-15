#[test]
fn test_test_getcwd() {
    assert_emscripten_output!(
        "../../emtests/test_getcwd.wasm",
        "test_getcwd",
        vec![],
        "../../emtests/test_getcwd.out"
    );
}
