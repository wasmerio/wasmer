#[test]
fn test_printf() {
    assert_emscripten_output!(
        "../../emtests/printf.wasm",
        "printf",
        vec![],
        "../../emtests/printf.out"
    );
}
