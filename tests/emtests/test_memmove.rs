#[test]
fn test_test_memmove() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_memmove.wasm",
        "test_memmove",
        vec![],
        "../emscripten_resources/emtests/test_memmove.out"
    );
}
