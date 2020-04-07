#[test]
#[ignore]
fn test_test_strtol_dec() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_strtol_dec.wasm",
        "test_strtol_dec",
        vec![],
        "../emscripten_resources/emtests/test_strtol_dec.out"
    );
}
