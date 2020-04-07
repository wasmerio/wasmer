#[test]
fn test_test_longjmp2() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_longjmp2.wasm",
        "test_longjmp2",
        vec![],
        "../emscripten_resources/emtests/test_longjmp2.out"
    );
}
