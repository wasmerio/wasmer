#[test]
#[ignore]
fn test_test_memcpy_memcmp() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_memcpy_memcmp.wasm",
        "test_memcpy_memcmp",
        vec![],
        "../emscripten_resources/emtests/test_memcpy_memcmp.out"
    );
}
