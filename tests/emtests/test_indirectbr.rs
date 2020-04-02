#[test]
fn test_test_indirectbr() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_indirectbr.wasm",
        "test_indirectbr",
        vec![],
        "../emscripten_resources/emtests/test_indirectbr.out"
    );
}
