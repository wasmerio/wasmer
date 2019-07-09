#[test]
fn test_test_loop() {
    assert_emscripten_output!(
        "../../emtests/test_loop.wasm",
        "test_loop",
        vec![],
        "../../emtests/test_loop.out"
    );
}
