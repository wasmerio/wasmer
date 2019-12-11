#[test]
#[ignore]
fn test_test_sscanf_other_whitespace() {
    assert_emscripten_output!(
        "../../emtests/test_sscanf_other_whitespace.wasm",
        "test_sscanf_other_whitespace",
        vec![],
        "../../emtests/test_sscanf_other_whitespace.out"
    );
}
