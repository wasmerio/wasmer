#[test]
fn test_test_frexp() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_frexp.wasm",
        "test_frexp",
        vec![],
        "../emscripten_resources/emtests/test_frexp.out"
    );
}
