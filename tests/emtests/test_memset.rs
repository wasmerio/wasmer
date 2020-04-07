#[test]
fn test_test_memset() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_memset.wasm",
        "test_memset",
        vec![],
        "../emscripten_resources/emtests/test_memset.out"
    );
}
