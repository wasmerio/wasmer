#[test]
fn test_test_i64_precise() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_i64_precise.wasm",
        "test_i64_precise",
        vec![],
        "../emscripten_resources/emtests/test_i64_precise.out"
    );
}
