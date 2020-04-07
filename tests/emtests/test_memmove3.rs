#[test]
fn test_test_memmove3() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_memmove3.wasm",
        "test_memmove3",
        vec![],
        "../emscripten_resources/emtests/test_memmove3.out"
    );
}
