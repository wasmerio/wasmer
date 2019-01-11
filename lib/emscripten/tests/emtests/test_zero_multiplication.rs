#[test]
#[ignore]
fn test_test_zero_multiplication() {
    assert_emscripten_output!(
        "../../emtests/test_zero_multiplication.wasm",
        "test_zero_multiplication",
        vec![],
        "../../emtests/test_zero_multiplication.out"
    );
}
