#[test]
#[ignore]
fn test_test_sscanf_whitespace() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_sscanf_whitespace.wasm",
        "test_sscanf_whitespace",
        vec![],
        "../emscripten_resources/emtests/test_sscanf_whitespace.out"
    );
}
