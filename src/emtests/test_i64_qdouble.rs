#[test]
fn test_test_i64_qdouble() {
    assert_emscripten_output!(
        "../../emtests/test_i64_qdouble.wasm",
        "test_i64_qdouble",
        vec![],
        "../../emtests/test_i64_qdouble.out"
    );
}
