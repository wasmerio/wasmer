#[test]
fn test_test_i64_i16() {
    assert_emscripten_output!(
        "../../emtests/test_i64_i16.wasm",
        "test_i64_i16",
        vec![],
        "../../emtests/test_i64_i16.out"
    );
}
