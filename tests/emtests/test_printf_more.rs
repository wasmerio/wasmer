#[test]
#[ignore]
fn test_test_printf_more() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_printf_more.wasm",
        "test_printf_more",
        vec![],
        "../emscripten_resources/emtests/test_printf_more.out"
    );
}
