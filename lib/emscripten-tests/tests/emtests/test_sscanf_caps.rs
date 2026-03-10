#[test]
#[ignore]
fn test_test_sscanf_caps() {
    assert_emscripten_output!(
        "../../emtests/test_sscanf_caps.wasm",
        "test_sscanf_caps",
        vec![],
        "../../emtests/test_sscanf_caps.out"
    );
}
