#[test]
#[ignore]
fn test_test_emscripten_api() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_emscripten_api.wasm",
        "test_emscripten_api",
        vec![],
        "../emscripten_resources/emtests/test_emscripten_api.out"
    );
}
