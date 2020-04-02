#[test]
fn test_test_isnan() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_isnan.wasm",
        "test_isnan",
        vec![],
        "../emscripten_resources/emtests/test_isnan.out"
    );
}
