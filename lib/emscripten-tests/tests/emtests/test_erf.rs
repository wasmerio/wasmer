#[test]
fn test_test_erf() {
    assert_emscripten_output!(
        "../../emtests/test_erf.wasm",
        "test_erf",
        vec![],
        "../../emtests/test_erf.out"
    );
}
