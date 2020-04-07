#[test]
#[ignore]
fn test_test_strtol_bin() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_strtol_bin.wasm",
        "test_strtol_bin",
        vec![],
        "../emscripten_resources/emtests/test_strtol_bin.out"
    );
}
