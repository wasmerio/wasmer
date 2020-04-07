#[test]
fn test_test_memcpy3() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_memcpy3.wasm",
        "test_memcpy3",
        vec![],
        "../emscripten_resources/emtests/test_memcpy3.out"
    );
}
