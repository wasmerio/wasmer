#[test]
fn test_test_flexarray_struct() {
    assert_emscripten_output!(
        "../../emtests/test_flexarray_struct.wasm",
        "test_flexarray_struct",
        vec![],
        "../../emtests/test_flexarray_struct.out"
    );
}
