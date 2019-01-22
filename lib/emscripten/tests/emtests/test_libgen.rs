#[test]
fn test_test_libgen() {
    assert_emscripten_output!(
        "../../emtests/test_libgen.wasm",
        "test_libgen",
        vec![],
        "../../emtests/test_libgen.out"
    );
}
