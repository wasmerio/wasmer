#[test]
#[ignore]
fn test_test_strndup() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_strndup.wasm",
        "test_strndup",
        vec![],
        "../emscripten_resources/emtests/test_strndup.out"
    );
}
