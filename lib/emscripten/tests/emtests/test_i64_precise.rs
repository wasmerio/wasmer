#[test]
fn test_test_i64_precise() {
    assert_emscripten_output!(
        "../../emtests/test_i64_precise.wasm",
        "test_i64_precise",
        vec![],
        "../../emtests/test_i64_precise.out"
    );
}
