#[test]
fn test_test_memmove2() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_memmove2.wasm",
        "test_memmove2",
        vec![],
        "../emscripten_resources/emtests/test_memmove2.out"
    );
}
