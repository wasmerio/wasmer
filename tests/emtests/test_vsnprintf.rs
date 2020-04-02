#[test]
#[ignore]
fn test_test_vsnprintf() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_vsnprintf.wasm",
        "test_vsnprintf",
        vec![],
        "../emscripten_resources/emtests/test_vsnprintf.out"
    );
}
