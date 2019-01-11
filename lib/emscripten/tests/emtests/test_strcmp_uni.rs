#[test]
fn test_test_strcmp_uni() {
    assert_emscripten_output!(
        "../../emtests/test_strcmp_uni.wasm",
        "test_strcmp_uni",
        vec![],
        "../../emtests/test_strcmp_uni.out"
    );
}
