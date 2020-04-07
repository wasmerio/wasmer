#[test]
fn test_test_longjmp_repeat() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_longjmp_repeat.wasm",
        "test_longjmp_repeat",
        vec![],
        "../emscripten_resources/emtests/test_longjmp_repeat.out"
    );
}
