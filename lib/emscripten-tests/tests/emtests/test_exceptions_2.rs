#[test]
#[ignore]
fn test_test_exceptions_2() {
    assert_emscripten_output!(
        "../../emtests/test_exceptions_2.wasm",
        "test_exceptions_2",
        vec![],
        "../../emtests/test_exceptions_2.out"
    );
}
