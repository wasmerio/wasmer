#[test]
#[ignore]
fn test_test_sscanf_skip() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_sscanf_skip.wasm",
        "test_sscanf_skip",
        vec![],
        "../emscripten_resources/emtests/test_sscanf_skip.out"
    );
}
