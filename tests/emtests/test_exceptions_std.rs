#[test]
#[ignore]
fn test_test_exceptions_std() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_exceptions_std.wasm",
        "test_exceptions_std",
        vec![],
        "../emscripten_resources/emtests/test_exceptions_std.out"
    );
}
