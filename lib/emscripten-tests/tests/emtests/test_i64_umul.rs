#[test]
fn test_test_i64_umul() {
    assert_emscripten_output!(
        "../../emtests/test_i64_umul.wasm",
        "test_i64_umul",
        vec![],
        "../../emtests/test_i64_umul.out"
    );
}
