#[test]
#[ignore]
fn test_test_exceptions_white_list() {
    assert_emscripten_output!(
        "../../emtests/test_exceptions_white_list.wasm",
        "test_exceptions_white_list",
        vec![],
        "../../emtests/test_exceptions_white_list.out"
    );
}
