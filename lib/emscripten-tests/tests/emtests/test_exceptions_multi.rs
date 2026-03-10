#[test]
#[ignore]
fn test_test_exceptions_multi() {
    assert_emscripten_output!(
        "../../emtests/test_exceptions_multi.wasm",
        "test_exceptions_multi",
        vec![],
        "../../emtests/test_exceptions_multi.out"
    );
}
