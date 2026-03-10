#[test]
#[ignore]
fn test_test_stack_void() {
    assert_emscripten_output!(
        "../../emtests/test_stack_void.wasm",
        "test_stack_void",
        vec![],
        "../../emtests/test_stack_void.out"
    );
}
