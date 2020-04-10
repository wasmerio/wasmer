#[test]
fn test_test_longjmp_throw() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_longjmp_throw.wasm",
        "test_longjmp_throw",
        vec![],
        "../emscripten_resources/emtests/test_longjmp_throw.out"
    );
}
