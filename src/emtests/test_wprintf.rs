#[test]
#[ignore]
fn test_test_wprintf() {
    assert_emscripten_output!(
        "../../emtests/test_wprintf.wasm",
        "test_wprintf",
        vec![],
        "../../emtests/test_wprintf.out"
    );
}
