#[test]
#[ignore]
fn test_test_tracing() {
    assert_emscripten_output!(
        "../../emtests/test_tracing.wasm",
        "test_tracing",
        vec![],
        "../../emtests/test_tracing.out"
    );
}
