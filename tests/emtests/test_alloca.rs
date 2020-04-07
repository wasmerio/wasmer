#[test]
fn test_test_alloca() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_alloca.wasm",
        "test_alloca",
        vec![],
        "../emscripten_resources/emtests/test_alloca.out"
    );
}
