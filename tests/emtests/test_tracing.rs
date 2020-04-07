#[test]
#[ignore]
fn test_test_tracing() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_tracing.wasm",
        "test_tracing",
        vec![],
        "../emscripten_resources/emtests/test_tracing.out"
    );
}
