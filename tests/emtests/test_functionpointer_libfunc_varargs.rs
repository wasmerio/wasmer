#[test]
fn test_test_functionpointer_libfunc_varargs() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_functionpointer_libfunc_varargs.wasm",
        "test_functionpointer_libfunc_varargs",
        vec![],
        "../emscripten_resources/emtests/test_functionpointer_libfunc_varargs.out"
    );
}
