#[test]
fn test_test_i32_mul_precise() {
    assert_emscripten_output!(
        "../../emtests/test_i32_mul_precise.wasm",
        "test_i32_mul_precise",
        vec![],
        "../../emtests/test_i32_mul_precise.out"
    );
}
