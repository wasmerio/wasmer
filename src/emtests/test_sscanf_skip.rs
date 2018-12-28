#[test]
#[ignore]
fn test_test_sscanf_skip() {
    assert_emscripten_output!(
        "../../emtests/test_sscanf_skip.wasm",
        "test_sscanf_skip",
        vec![],
        "../../emtests/test_sscanf_skip.out"
    );
}
