#[test]
fn test_test_longjmp3() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_longjmp3.wasm",
        "test_longjmp3",
        vec![],
        "../emscripten_resources/emtests/test_longjmp3.out"
    );
}
