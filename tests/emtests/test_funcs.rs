#[test]
fn test_test_funcs() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_funcs.wasm",
        "test_funcs",
        vec![],
        "../emscripten_resources/emtests/test_funcs.out"
    );
}
