#[test]
fn test_test_longjmp4() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_longjmp4.wasm",
        "test_longjmp4",
        vec![],
        "../emscripten_resources/emtests/test_longjmp4.out"
    );
}
