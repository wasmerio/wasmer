#[test]
#[ignore]
fn test_test_strtol_hex() {
    assert_emscripten_output!(
        "../../emtests/test_strtol_hex.wasm",
        "test_strtol_hex",
        vec![],
        "../../emtests/test_strtol_hex.out"
    );
}
