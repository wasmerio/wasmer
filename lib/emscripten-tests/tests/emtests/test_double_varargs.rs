#[test]
fn test_test_double_varargs() {
    assert_emscripten_output!(
        "../../emtests/test_double_varargs.wasm",
        "test_double_varargs",
        vec![],
        "../../emtests/test_double_varargs.out"
    );
}
