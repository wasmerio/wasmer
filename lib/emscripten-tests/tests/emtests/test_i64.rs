#[test]
#[ignore]
fn test_test_i64() {
    assert_emscripten_output!(
        "../../emtests/test_i64.wasm",
        "test_i64",
        vec![],
        "../../emtests/test_i64.out"
    );
}
