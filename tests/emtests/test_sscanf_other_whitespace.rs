#[test]
#[ignore]
fn test_test_sscanf_other_whitespace() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_sscanf_other_whitespace.wasm",
        "test_sscanf_other_whitespace",
        vec![],
        "../emscripten_resources/emtests/test_sscanf_other_whitespace.out"
    );
}
