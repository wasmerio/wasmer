#[test]
fn test_test_longjmp_stacked() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_longjmp_stacked.wasm",
        "test_longjmp_stacked",
        vec![],
        "../emscripten_resources/emtests/test_longjmp_stacked.out"
    );
}
