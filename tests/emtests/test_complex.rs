#[test]
fn test_test_complex() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_complex.wasm",
        "test_complex",
        vec![],
        "../emscripten_resources/emtests/test_complex.out"
    );
}
