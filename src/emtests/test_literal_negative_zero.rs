#[test]
fn test_test_literal_negative_zero() {
    assert_emscripten_output!(
        "../../emtests/test_literal_negative_zero.wasm",
        "test_literal_negative_zero",
        vec![],
        "../../emtests/test_literal_negative_zero.out"
    );
}
