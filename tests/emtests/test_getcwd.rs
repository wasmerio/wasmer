#[test]
fn test_test_getcwd() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_getcwd.wasm",
        "test_getcwd",
        vec![],
        "../emscripten_resources/emtests/test_getcwd.out"
    );
}
