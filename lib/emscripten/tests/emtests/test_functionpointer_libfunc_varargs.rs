#[test]
fn test_test_functionpointer_libfunc_varargs() {
    assert_emscripten_output!(
        "../../emtests/test_functionpointer_libfunc_varargs.wasm",
        "test_functionpointer_libfunc_varargs",
        vec![],
        "../../emtests/test_functionpointer_libfunc_varargs.out"
    );
}
