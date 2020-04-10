#[test]
#[ignore]
fn test_test_strcmp_uni() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_strcmp_uni.wasm",
        "test_strcmp_uni",
        vec![],
        "../emscripten_resources/emtests/test_strcmp_uni.out"
    );
}
