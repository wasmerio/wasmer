#[test]
#[ignore]
fn test_test_exceptions_white_list() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_exceptions_white_list.wasm",
        "test_exceptions_white_list",
        vec![],
        "../emscripten_resources/emtests/test_exceptions_white_list.out"
    );
}
