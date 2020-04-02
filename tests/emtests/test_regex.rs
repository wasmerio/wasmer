#[test]
#[ignore]
fn test_test_regex() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_regex.wasm",
        "test_regex",
        vec![],
        "../emscripten_resources/emtests/test_regex.out"
    );
}
