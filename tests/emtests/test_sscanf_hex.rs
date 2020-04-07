#[test]
#[ignore]
fn test_test_sscanf_hex() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_sscanf_hex.wasm",
        "test_sscanf_hex",
        vec![],
        "../emscripten_resources/emtests/test_sscanf_hex.out"
    );
}
