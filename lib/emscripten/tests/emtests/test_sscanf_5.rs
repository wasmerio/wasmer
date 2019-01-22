#[test]
fn test_test_sscanf_5() {
    assert_emscripten_output!(
        "../../emtests/test_sscanf_5.wasm",
        "test_sscanf_5",
        vec![],
        "../../emtests/test_sscanf_5.out"
    );
}
