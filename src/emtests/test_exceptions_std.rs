#[test]
#[ignore]
fn test_test_exceptions_std() {
    assert_emscripten_output!(
        "../../emtests/test_exceptions_std.wasm",
        "test_exceptions_std",
        vec![],
        "../../emtests/test_exceptions_std.out"
    );
}
