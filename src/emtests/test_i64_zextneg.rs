#[test]
fn test_test_i64_zextneg() {
    assert_emscripten_output!(
        "../../emtests/test_i64_zextneg.wasm",
        "test_i64_zextneg",
        vec![],
        "../../emtests/test_i64_zextneg.out"
    );
}
