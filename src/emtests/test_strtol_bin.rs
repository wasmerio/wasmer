#[test]
fn test_test_strtol_bin() {
    assert_emscripten_output!(
        "../../emtests/test_strtol_bin.wasm",
        "test_strtol_bin",
        vec![],
        "../../emtests/test_strtol_bin.out"
    );
}
