#[test]
fn test_test_nested_struct_varargs() {
    assert_emscripten_output!(
        "../../emtests/test_nested_struct_varargs.wasm",
        "test_nested_struct_varargs",
        vec![],
        "../../emtests/test_nested_struct_varargs.out"
    );
}
