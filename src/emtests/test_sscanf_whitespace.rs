#[test]
#[ignore]
fn test_test_sscanf_whitespace() {
    assert_emscripten_output!(
        "../../emtests/test_sscanf_whitespace.wasm",
        "test_sscanf_whitespace",
        vec![],
        "../../emtests/test_sscanf_whitespace.out"
    );
}
