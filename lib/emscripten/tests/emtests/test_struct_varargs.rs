#[test]
fn test_test_struct_varargs() {
    assert_emscripten_output!(
        "../../emtests/test_struct_varargs.wasm",
        "test_struct_varargs",
        vec![],
        "../../emtests/test_struct_varargs.out"
    );
}
