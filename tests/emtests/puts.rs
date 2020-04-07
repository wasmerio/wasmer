#[test]
fn test_puts() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/puts.wasm",
        "puts",
        vec![],
        "../emscripten_resources/emtests/puts.out"
    );
}
