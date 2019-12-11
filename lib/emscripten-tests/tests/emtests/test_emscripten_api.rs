#[test]
#[ignore]
fn test_test_emscripten_api() {
    assert_emscripten_output!(
        "../../emtests/test_emscripten_api.wasm",
        "test_emscripten_api",
        vec![],
        "../../emtests/test_emscripten_api.out"
    );
}
