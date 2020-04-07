#[test]
#[ignore]
fn test_test_strtol_oct() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_strtol_oct.wasm",
        "test_strtol_oct",
        vec![],
        "../emscripten_resources/emtests/test_strtol_oct.out"
    );
}
