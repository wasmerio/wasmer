#[test]
fn test_test_alloca_stack() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_alloca_stack.wasm",
        "test_alloca_stack",
        vec![],
        "../emscripten_resources/emtests/test_alloca_stack.out"
    );
}
