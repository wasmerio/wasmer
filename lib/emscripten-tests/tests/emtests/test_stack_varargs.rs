#[test]
#[ignore]
fn test_test_stack_varargs() {
    assert_emscripten_output!(
        "../../emtests/test_stack_varargs.wasm",
        "test_stack_varargs",
        vec![],
        "../../emtests/test_stack_varargs.out"
    );
}
