#[test]
#[ignore]
fn test_test_struct_varargs() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_struct_varargs.wasm",
        "test_struct_varargs",
        vec![],
        "../emscripten_resources/emtests/test_struct_varargs.out"
    );
}
