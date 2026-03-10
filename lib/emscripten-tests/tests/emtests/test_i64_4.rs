#[test]
fn test_test_i64_4() {
    assert_emscripten_output!(
        "../../emtests/test_i64_4.wasm",
        "test_i64_4",
        vec![],
        "../../emtests/test_i64_4.out"
    );
}
