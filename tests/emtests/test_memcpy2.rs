#[test]
fn test_test_memcpy2() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_memcpy2.wasm",
        "test_memcpy2",
        vec![],
        "../emscripten_resources/emtests/test_memcpy2.out"
    );
}
