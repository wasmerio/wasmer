#[test]
fn test_test_if() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_if.wasm",
        "test_if",
        vec![],
        "../emscripten_resources/emtests/test_if.out"
    );
}
