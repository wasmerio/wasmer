#[test]
#[ignore]
fn test_test_strings() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_strings.wasm",
        "test_strings",
        vec![],
        "../emscripten_resources/emtests/test_strings.out"
    );
}
