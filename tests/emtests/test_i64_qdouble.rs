#[test]
fn test_test_i64_qdouble() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_i64_qdouble.wasm",
        "test_i64_qdouble",
        vec![],
        "../emscripten_resources/emtests/test_i64_qdouble.out"
    );
}
