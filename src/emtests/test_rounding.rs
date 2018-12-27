#[test]
fn test_test_rounding() {
    assert_emscripten_output!(
        "../../emtests/test_rounding.wasm",
        "test_rounding",
        vec![],
        "../../emtests/test_rounding.out"
    );
}
