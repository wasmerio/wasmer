#[test]
fn test_test_llrint() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_llrint.wasm",
        "test_llrint",
        vec![],
        "../emscripten_resources/emtests/test_llrint.out"
    );
}
