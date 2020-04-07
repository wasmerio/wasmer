#[test]
#[ignore]
fn test_test_zero_multiplication() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_zero_multiplication.wasm",
        "test_zero_multiplication",
        vec![],
        "../emscripten_resources/emtests/test_zero_multiplication.out"
    );
}
