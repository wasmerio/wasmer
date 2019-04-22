#[test]
#[ignore]
fn test_test_printf_2() {
    assert_emscripten_output!(
        "../../emtests/test_printf_2.wasm",
        "test_printf_2",
        vec![],
        "../../emtests/test_printf_2.out"
    );
}
