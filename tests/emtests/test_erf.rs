#[test]
fn test_test_erf() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_erf.wasm",
        "test_erf",
        vec![],
        "../emscripten_resources/emtests/test_erf.out"
    );
}
