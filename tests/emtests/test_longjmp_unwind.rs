#[test]
fn test_test_longjmp_unwind() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_longjmp_unwind.wasm",
        "test_longjmp_unwind",
        vec![],
        "../emscripten_resources/emtests/test_longjmp_unwind.out"
    );
}
