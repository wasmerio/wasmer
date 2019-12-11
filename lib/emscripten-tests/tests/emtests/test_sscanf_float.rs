#[test]
#[ignore]
fn test_test_sscanf_float() {
    assert_emscripten_output!(
        "../../emtests/test_sscanf_float.wasm",
        "test_sscanf_float",
        vec![],
        "../../emtests/test_sscanf_float.out"
    );
}
