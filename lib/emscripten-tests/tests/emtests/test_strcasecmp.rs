#[test]
#[ignore]
fn test_test_strcasecmp() {
    assert_emscripten_output!(
        "../../emtests/test_strcasecmp.wasm",
        "test_strcasecmp",
        vec![],
        "../../emtests/test_strcasecmp.out"
    );
}
