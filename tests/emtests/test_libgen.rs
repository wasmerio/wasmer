#[test]
fn test_test_libgen() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_libgen.wasm",
        "test_libgen",
        vec![],
        "../emscripten_resources/emtests/test_libgen.out"
    );
}
