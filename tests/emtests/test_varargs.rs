#[test]
#[ignore]
fn test_test_varargs() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_varargs.wasm",
        "test_varargs",
        vec![],
        "../emscripten_resources/emtests/test_varargs.out"
    );
}
