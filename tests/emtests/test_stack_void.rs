#[test]
#[ignore]
fn test_test_stack_void() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_stack_void.wasm",
        "test_stack_void",
        vec![],
        "../emscripten_resources/emtests/test_stack_void.out"
    );
}
