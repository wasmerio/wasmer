#[test]
fn test_test_if_else() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_if_else.wasm",
        "test_if_else",
        vec![],
        "../emscripten_resources/emtests/test_if_else.out"
    );
}
