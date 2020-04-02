#[test]
fn test_test_loop() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_loop.wasm",
        "test_loop",
        vec![],
        "../emscripten_resources/emtests/test_loop.out"
    );
}
