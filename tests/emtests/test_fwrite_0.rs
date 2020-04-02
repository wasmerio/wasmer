#[test]
fn test_test_fwrite_0() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_fwrite_0.wasm",
        "test_fwrite_0",
        vec![],
        "../emscripten_resources/emtests/test_fwrite_0.out"
    );
}
