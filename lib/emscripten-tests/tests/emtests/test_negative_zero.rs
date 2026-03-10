#[test]
fn test_test_negative_zero() {
    assert_emscripten_output!(
        "../../emtests/test_negative_zero.wasm",
        "test_negative_zero",
        vec![],
        "../../emtests/test_negative_zero.out"
    );
}
