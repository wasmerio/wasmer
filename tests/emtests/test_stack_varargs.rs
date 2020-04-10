#[test]
#[ignore]
fn test_test_stack_varargs() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_stack_varargs.wasm",
        "test_stack_varargs",
        vec![],
        "../emscripten_resources/emtests/test_stack_varargs.out"
    );
}
