#[test]
fn test_test_nested_struct_varargs() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_nested_struct_varargs.wasm",
        "test_nested_struct_varargs",
        vec![],
        "../emscripten_resources/emtests/test_nested_struct_varargs.out"
    );
}
