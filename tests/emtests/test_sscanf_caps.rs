#[test]
#[ignore]
fn test_test_sscanf_caps() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_sscanf_caps.wasm",
        "test_sscanf_caps",
        vec![],
        "../emscripten_resources/emtests/test_sscanf_caps.out"
    );
}
