#[test]
#[ignore]
fn test_test_strcasecmp() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_strcasecmp.wasm",
        "test_strcasecmp",
        vec![],
        "../emscripten_resources/emtests/test_strcasecmp.out"
    );
}
