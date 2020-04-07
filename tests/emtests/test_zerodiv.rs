#[test]
#[ignore]
fn test_test_zerodiv() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_zerodiv.wasm",
        "test_zerodiv",
        vec![],
        "../emscripten_resources/emtests/test_zerodiv.out"
    );
}
