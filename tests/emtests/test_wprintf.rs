#[test]
#[ignore]
fn test_test_wprintf() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_wprintf.wasm",
        "test_wprintf",
        vec![],
        "../emscripten_resources/emtests/test_wprintf.out"
    );
}
