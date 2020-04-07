#[test]
#[ignore]
fn test_test_rounding() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_rounding.wasm",
        "test_rounding",
        vec![],
        "../emscripten_resources/emtests/test_rounding.out"
    );
}
