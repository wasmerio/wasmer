#[test]
#[ignore]
fn test_test_sscanf_hex() {
    assert_emscripten_output!(
        "../../emtests/test_sscanf_hex.wasm",
        "test_sscanf_hex",
        vec![],
        "../../emtests/test_sscanf_hex.out"
    );
}
