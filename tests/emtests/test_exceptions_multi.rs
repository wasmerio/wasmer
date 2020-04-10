#[test]
#[ignore]
fn test_test_exceptions_multi() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_exceptions_multi.wasm",
        "test_exceptions_multi",
        vec![],
        "../emscripten_resources/emtests/test_exceptions_multi.out"
    );
}
