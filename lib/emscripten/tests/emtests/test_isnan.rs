#[test]
fn test_test_isnan() {
    assert_emscripten_output!(
        "../../emtests/test_isnan.wasm",
        "test_isnan",
        vec![],
        "../../emtests/test_isnan.out"
    );
}
