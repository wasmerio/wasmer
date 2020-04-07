#[test]
fn test_printf() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/printf.wasm",
        "printf",
        vec![],
        "../emscripten_resources/emtests/printf.out"
    );
}
