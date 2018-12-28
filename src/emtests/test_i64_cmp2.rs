#[test]
fn test_test_i64_cmp2() {
    assert_emscripten_output!(
        "../../emtests/test_i64_cmp2.wasm",
        "test_i64_cmp2",
        vec![],
        "../../emtests/test_i64_cmp2.out"
    );
}
