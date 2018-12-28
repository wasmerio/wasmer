#[test]
fn test_test_strtol_dec() {
    assert_emscripten_output!(
        "../../emtests/test_strtol_dec.wasm",
        "test_strtol_dec",
        vec![],
        "../../emtests/test_strtol_dec.out"
    );
}
