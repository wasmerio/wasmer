#[test]
fn test_test_alloca_stack() {
    assert_emscripten_output!(
        "../../emtests/test_alloca_stack.wasm",
        "test_alloca_stack",
        vec![],
        "../../emtests/test_alloca_stack.out"
    );
}
