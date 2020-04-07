#[test]
#[ignore]
fn test_test_exceptions_2() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_exceptions_2.wasm",
        "test_exceptions_2",
        vec![],
        "../emscripten_resources/emtests/test_exceptions_2.out"
    );
}
