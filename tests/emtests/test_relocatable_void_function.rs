#[test]
#[ignore]
fn test_test_relocatable_void_function() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_relocatable_void_function.wasm",
        "test_relocatable_void_function",
        vec![],
        "../emscripten_resources/emtests/test_relocatable_void_function.out"
    );
}
