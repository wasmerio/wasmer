#[test]
fn test_test_flexarray_struct() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_flexarray_struct.wasm",
        "test_flexarray_struct",
        vec![],
        "../emscripten_resources/emtests/test_flexarray_struct.out"
    );
}
