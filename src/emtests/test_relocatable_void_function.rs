#[test]
fn test_test_relocatable_void_function() {
    assert_emscripten_output!(
        "../../emtests/test_relocatable_void_function.wasm",
        "test_relocatable_void_function",
        vec![],
        "../../emtests/test_relocatable_void_function.out"
    );
}
