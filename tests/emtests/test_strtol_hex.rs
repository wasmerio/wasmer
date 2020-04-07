#[test]
#[ignore]
fn test_test_strtol_hex() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_strtol_hex.wasm",
        "test_strtol_hex",
        vec![],
        "../emscripten_resources/emtests/test_strtol_hex.out"
    );
}
