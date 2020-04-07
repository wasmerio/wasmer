#[test]
fn test_test_i64_4() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_i64_4.wasm",
        "test_i64_4",
        vec![],
        "../emscripten_resources/emtests/test_i64_4.out"
    );
}
