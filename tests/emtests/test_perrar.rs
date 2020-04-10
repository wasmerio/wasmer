#[test]
#[ignore]
fn test_test_perrar() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_perrar.wasm",
        "test_perrar",
        vec![],
        "../emscripten_resources/emtests/test_perrar.out"
    );
}
