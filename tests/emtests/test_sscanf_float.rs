#[test]
#[ignore]
fn test_test_sscanf_float() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_sscanf_float.wasm",
        "test_sscanf_float",
        vec![],
        "../emscripten_resources/emtests/test_sscanf_float.out"
    );
}
