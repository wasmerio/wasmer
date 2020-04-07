#[test]
#[ignore]
fn test_test_sintvars() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_sintvars.wasm",
        "test_sintvars",
        vec![],
        "../emscripten_resources/emtests/test_sintvars.out"
    );
}
