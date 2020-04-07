#[test]
#[ignore]
fn test_test_vprintf() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_vprintf.wasm",
        "test_vprintf",
        vec![],
        "../emscripten_resources/emtests/test_vprintf.out"
    );
}
