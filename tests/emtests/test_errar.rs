#[test]
fn test_test_errar() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_errar.wasm",
        "test_errar",
        vec![],
        "../emscripten_resources/emtests/test_errar.out"
    );
}
