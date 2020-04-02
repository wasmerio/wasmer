#[test]
fn test_test_i64_i16() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_i64_i16.wasm",
        "test_i64_i16",
        vec![],
        "../emscripten_resources/emtests/test_i64_i16.out"
    );
}
