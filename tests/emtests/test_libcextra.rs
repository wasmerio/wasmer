#[test]
fn test_test_libcextra() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_libcextra.wasm",
        "test_libcextra",
        vec![],
        "../emscripten_resources/emtests/test_libcextra.out"
    );
}
