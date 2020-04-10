#[test]
#[ignore]
fn test_test_phiundef() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_phiundef.wasm",
        "test_phiundef",
        vec![],
        "../emscripten_resources/emtests/test_phiundef.out"
    );
}
