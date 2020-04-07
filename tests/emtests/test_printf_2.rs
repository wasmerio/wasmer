#[test]
#[ignore]
fn test_test_printf_2() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_printf_2.wasm",
        "test_printf_2",
        vec![],
        "../emscripten_resources/emtests/test_printf_2.out"
    );
}
