#[test]
fn test_test_i32_mul_precise() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_i32_mul_precise.wasm",
        "test_i32_mul_precise",
        vec![],
        "../emscripten_resources/emtests/test_i32_mul_precise.out"
    );
}
