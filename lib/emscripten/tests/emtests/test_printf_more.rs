#[test]
fn test_test_printf_more() {
    assert_emscripten_output!(
        "../../emtests/test_printf_more.wasm",
        "test_printf_more",
        vec![],
        "../../emtests/test_printf_more.out"
    );
}
