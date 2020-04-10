#[test]
fn test_test_double_varargs() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_double_varargs.wasm",
        "test_double_varargs",
        vec![],
        "../emscripten_resources/emtests/test_double_varargs.out"
    );
}
