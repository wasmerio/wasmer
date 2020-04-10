#[test]
fn test_test_longjmp() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_longjmp.wasm",
        "test_longjmp",
        vec![],
        "../emscripten_resources/emtests/test_longjmp.out"
    );
}
